//! Wave-parallel incremental scheduler.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;
use crate::graph::{Graph, CollectionElement, Endpoint};
use crate::loader::Loader;
use crate::node_id::NodeId;
use crate::transform::{
    TransformContext, TransformError, ContextOutput,
};
use crate::value::ValueTypeRegistry;

// ---------------------------------------------------------------------------
// UpdateReport (public)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct UpdateReport {
    pub transforms_evaluated:        usize,
    pub transforms_changed:          usize,
    pub transforms_skipped:          usize,
    pub transforms_blocked:          usize,
    pub collection_elements_changed: usize,
    pub errors:                      Vec<(Uuid, TransformError)>,
    pub cycle_limit_exceeded:        Vec<Uuid>,
}

impl UpdateReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty() && self.cycle_limit_exceeded.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

pub(crate) struct Scheduler {
    graph:       Arc<Graph>,
    loader:      Arc<Loader>,
    registry:    Arc<ValueTypeRegistry>,
    cycle_limit: u32,
}

impl Scheduler {
    pub(crate) fn new(
        graph:    Arc<Graph>,
        loader:   Arc<Loader>,
        registry: Arc<ValueTypeRegistry>,
    ) -> Self {
        Self { graph, loader, registry, cycle_limit: 1000 }
    }

    pub(crate) fn set_cycle_limit(&mut self, limit: u32) { self.cycle_limit = limit; }

    pub(crate) async fn run_update(&self) -> UpdateReport {
        let mut report = UpdateReport::default();
        let mut iteration = 0u32;

        loop {
            let dirty = self.graph.dirty_transforms_topo();
            if dirty.is_empty() { break; }

            if iteration >= self.cycle_limit {
                // Cycle limit exceeded: record the remaining dirty nodes and bail.
                for id in dirty {
                    report.cycle_limit_exceeded.push(id.as_uuid());
                }
                break;
            }
            iteration += 1;

            let waves = compute_waves(&dirty, &self.graph);

            for wave in waves {
                let mut handles = vec![];
                for tid in wave {
                    let graph    = Arc::clone(&self.graph);
                    let loader   = Arc::clone(&self.loader);
                    let registry = Arc::clone(&self.registry);
                    handles.push(tokio::spawn(async move {
                        evaluate_transform(tid, graph, loader, registry).await
                    }));
                }
                for handle in handles {
                    match handle.await {
                        Ok(r) => merge_result(&mut report, r),
                        Err(e) => {
                            report.errors.push((Uuid::nil(), TransformError::new(e.to_string())));
                        }
                    }
                }
            }
        }

        report
    }
}

// ---------------------------------------------------------------------------
// Wave computation
// ---------------------------------------------------------------------------

fn compute_waves(nodes: &[NodeId], graph: &Arc<Graph>) -> Vec<Vec<NodeId>> {
    let dirty_set: HashSet<NodeId> = nodes.iter().cloned().collect();
    let mut wave_map: HashMap<NodeId, usize> = HashMap::new();

    for &id in nodes {
        let preds = get_dirty_predecessors(id, &dirty_set, graph);
        let w = preds.iter()
            .map(|p| wave_map.get(p).copied().unwrap_or(0) + 1)
            .max()
            .unwrap_or(0);
        wave_map.insert(id, w);
    }

    let max_wave = wave_map.values().copied().max().unwrap_or(0);
    let mut waves: Vec<Vec<NodeId>> = vec![vec![]; max_wave + 1];
    for (&id, &w) in &wave_map { waves[w].push(id); }
    waves.retain(|w| !w.is_empty());
    waves
}

fn get_dirty_predecessors(
    id: NodeId,
    dirty_set: &HashSet<NodeId>,
    graph: &Arc<Graph>,
) -> Vec<NodeId> {
    let schema = match graph.transform_schema(id) { Some(s) => s, None => return vec![] };
    let mut preds = vec![];
    for slot in 0..schema.inputs.len() {
        for src in graph.input_slot_sources(id, slot) {
            if dirty_set.contains(&src) { preds.push(src); }
        }
    }
    preds
}

// ---------------------------------------------------------------------------
// Transform evaluation
// ---------------------------------------------------------------------------

struct EvalResult {
    node_id:      NodeId,
    changed:      bool,
    coll_changed: usize,
    error:        Option<TransformError>,
}

async fn evaluate_transform(
    tid:      NodeId,
    graph:    Arc<Graph>,
    loader:   Arc<Loader>,
    registry: Arc<ValueTypeRegistry>,
) -> EvalResult {
    let erased = match graph.get_erased_transform(tid) {
        Some(t) => t,
        None => return err_result(tid, format!("no transform for {tid}")),
    };

    let schema = Arc::clone(&erased.schema);
    let n_in   = schema.inputs.len();

    // Detect fan-out: Collection→Single crossing on any input slot.
    let is_fanout = (0..n_in).any(|slot| {
        !schema.inputs[slot].is_col && graph.input_slot_is_collection(tid, slot)
    });

    if is_fanout {
        return evaluate_fanout(tid, erased, graph, loader, registry).await;
    }

    // Normal invocation.
    let mut ctx = TransformContext::new(Arc::clone(&schema), Arc::clone(&registry));

    for slot in 0..n_in {
        let spec = &schema.inputs[slot];
        if spec.is_col {
            let elems = graph.read_collection_input(tid, slot);
            if let Some(build) = spec.build_collection {
                let all_values: Vec<_>  = elems.iter().map(|e| e.value.clone()).collect();
                let dirty_keys: Vec<u64> = elems.iter().filter(|e| e.dirty).map(|e| e.key).collect();
                ctx.collection_inputs[slot] = Some(build(all_values, dirty_keys, &registry));
            }
        } else {
            let (v, _h) = match graph.read_single_input(tid, slot) {
                Some(x) => x,
                None => return err_result(tid, format!("missing input on slot {slot}")),
            };
            match (spec.downcast)(&v, &registry) {
                Ok(boxed) => ctx.single_inputs[slot] = Some(boxed),
                Err(e) => return err_result(tid, format!("slot {slot} downcast: {e}")),
            }
        }
    }

    run_ctx_and_push(tid, erased, ctx, graph, loader, registry).await
}

/// Fan-out: invoke once per dirty Collection→Single element.
async fn evaluate_fanout(
    tid:      NodeId,
    erased:   crate::transform::ErasedTransform,
    graph:    Arc<Graph>,
    loader:   Arc<Loader>,
    registry: Arc<ValueTypeRegistry>,
) -> EvalResult {
    let schema = Arc::clone(&erased.schema);

    let fanout_slot = schema.inputs.iter().enumerate()
        .position(|(i, s)| !s.is_col && graph.input_slot_is_collection(tid, i))
        .unwrap_or(0);

    let all_elements = graph.read_collection_input(tid, fanout_slot);
    let current_input_keys: HashSet<u64> = all_elements.iter().map(|e| e.key).collect();
    let dirty_elems: Vec<CollectionElement> =
        all_elements.into_iter().filter(|e| e.dirty).collect();

    let n_out = schema.outputs.len();

    // Determine removed keys: keys in the output collection that are no longer
    // present in the input collection.  These must be removed from all output slots.
    let mut any_changed = false;
    let mut any_removed = false;
    for slot in 0..n_out {
        let output_keys: Vec<u64> = graph.get_collection_output_keys(tid, slot);
        let removed_keys: Vec<u64> = output_keys.into_iter()
            .filter(|k| !current_input_keys.contains(k))
            .collect();
        if !removed_keys.is_empty() {
            graph.remove_collection_elements(tid, slot, &removed_keys);
            any_removed = true;
            any_changed = true;
        }
    }

    if dirty_elems.is_empty() {
        graph.clear_input_dirty(tid, fanout_slot);
        if any_removed {
            // Downstream was already dirtied by remove_collection_elements.
            graph.mark_clean(tid);
            return ok_result(tid, true, 0);
        }
        graph.mark_clean(tid);
        return ok_result(tid, false, 0);
    }

    let fanout_spec = schema.inputs[fanout_slot].clone();
    let mut changed = any_changed;
    let mut coll_changed = 0;
    let mut last_error: Option<TransformError> = None;

    // Accumulate outputs per-output-slot.
    let mut accumulated: Vec<Vec<(u64, crate::value::Value, crate::value::ValueHash)>> =
        (0..n_out).map(|_| Vec::new()).collect();

    for elem in &dirty_elems {
        let boxed = match (fanout_spec.downcast)(&elem.value, &registry) {
            Ok(b) => b,
            Err(e) => { last_error = Some(TransformError::new(e)); continue; }
        };

        let mut ctx = TransformContext::new(Arc::clone(&schema), Arc::clone(&registry));
        ctx.single_inputs[fanout_slot] = Some(boxed);

        // Fill other non-collection, non-fanout inputs.
        let mut ok = true;
        for (slot, spec) in schema.inputs.iter().enumerate() {
            if slot == fanout_slot || spec.is_col { continue; }
            match graph.read_single_input(tid, slot) {
                Some((v, _)) => match (spec.downcast)(&v, &registry) {
                    Ok(b) => ctx.single_inputs[slot] = Some(b),
                    Err(e) => { last_error = Some(TransformError::new(e)); ok = false; break; }
                },
                None => {
                    last_error = Some(TransformError::new(format!("missing input slot {slot}")));
                    ok = false; break;
                }
            }
        }
        if !ok { continue; }

        if let Err(e) = erased.apply(&mut ctx).await {
            last_error = Some(e);
            continue;
        }

        // Accumulate single output as a collection element keyed by elem.key.
        for (slot, opt_out) in ctx.outputs.into_iter().enumerate() {
            if let Some(ContextOutput::Single(v, h)) = opt_out {
                accumulated[slot].push((elem.key, v, h));
            }
        }
    }

    // Upsert accumulated outputs per slot (preserves elements not in the dirty set).
    for (slot, items) in accumulated.into_iter().enumerate() {
        if items.is_empty() { continue; }
        // Persist elements to storage.
        for (key, v, h) in &items {
            for (_eid, endpoint) in graph.output_edge_targets(tid, slot) {
                if let Endpoint::Io(nid) = endpoint {
                    let _ = loader.persist_element(nid, *key, v, *h).await;
                }
            }
        }
        if graph.upsert_collection_output(tid, slot, items) {
            changed = true;
            coll_changed += 1;
        }
    }

    graph.clear_input_dirty(tid, fanout_slot);

    if let Some(e) = last_error {
        graph.mark_error(tid, e.clone());
        return EvalResult { node_id: tid, changed, coll_changed, error: Some(e) };
    }
    graph.mark_clean(tid);
    ok_result(tid, changed, coll_changed)
}

/// Run ctx through erased.apply(), push outputs, return result.
async fn run_ctx_and_push(
    tid:      NodeId,
    erased:   crate::transform::ErasedTransform,
    mut ctx:  TransformContext,
    graph:    Arc<Graph>,
    loader:   Arc<Loader>,
    registry: Arc<ValueTypeRegistry>,
) -> EvalResult {
    if let Err(e) = erased.apply(&mut ctx).await {
        graph.mark_error(tid, e.clone());
        return EvalResult { node_id: tid, changed: false, coll_changed: 0, error: Some(e) };
    }

    let mut changed = false;
    let mut coll_changed = 0usize;

    for (slot, opt_out) in ctx.outputs.into_iter().enumerate() {
        match opt_out {
            None => {}
            Some(ContextOutput::Single(v, h)) => {
                if graph.push_single_output(tid, slot, v.clone(), h) {
                    changed = true;
                    for (_eid, endpoint) in graph.output_edge_targets(tid, slot) {
                        if let Endpoint::Io(nid) = endpoint {
                            graph.store_output_on_node(nid, v.clone(), h);
                            let _ = loader.persist(nid, &v, h).await;
                        }
                    }
                }
            }
            Some(ContextOutput::Collection(pairs)) => {
                let n = pairs.len();
                let items: Vec<_> = pairs.into_iter().collect();
                if graph.push_collection_output(tid, slot, items.clone()) {
                    changed = true;
                    coll_changed += n;
                    for (_eid, endpoint) in graph.output_edge_targets(tid, slot) {
                        if let Endpoint::Io(nid) = endpoint {
                            for (key, v, h) in &items {
                                let _ = loader.persist_element(nid, *key, v, *h).await;
                            }
                        }
                    }
                }
            }
        }
    }

    graph.mark_clean(tid);
    ok_result(tid, changed, coll_changed)
}

fn err_result(id: NodeId, msg: impl Into<String>) -> EvalResult {
    EvalResult { node_id: id, changed: false, coll_changed: 0, error: Some(TransformError::new(msg)) }
}
fn ok_result(id: NodeId, changed: bool, coll_changed: usize) -> EvalResult {
    EvalResult { node_id: id, changed, coll_changed, error: None }
}

fn merge_result(report: &mut UpdateReport, r: EvalResult) {
    report.transforms_evaluated += 1;
    if r.error.is_some() {
        report.errors.push((r.node_id.as_uuid(), r.error.unwrap()));
    } else if r.changed {
        report.transforms_changed += 1;
        report.collection_elements_changed += r.coll_changed;
    } else {
        report.transforms_skipped += 1;
    }
}