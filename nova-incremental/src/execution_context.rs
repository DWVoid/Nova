//! ExecutionContext + TaskGraph: bridges Topology/WorkState and Scheduler.
//!
//! ExecutionContext owns the WorkState and Loader.
//! It builds a TaskGraph from the current dirty set, and applies task results
//! back into WorkState after each scheduler wave.
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;
use crate::loader::Loader;
use crate::node_id::NodeId;
use crate::topology::{Topology, EdgeId, Endpoint};
use crate::transform::{
    ErasedTransform, TransformContext, TransformError, ContextOutput,
};
use crate::value::{Value, ValueHash, ValueTypeRegistry};
use crate::workstate::{WorkState, CollectionElement};
// ---------------------------------------------------------------------------
// OutputSlot / TaskResult
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub(crate) enum OutputSlot {
    Single(Value, ValueHash),
    Collection(Vec<(u64, Value, ValueHash)>),
}
/// Result returned by a task after execution.
pub(crate) struct TaskResult {
    pub(crate) id:       NodeId,
    pub(crate) outputs:  Vec<Option<OutputSlot>>,
    pub(crate) error:    Option<TransformError>,
    /// For fan-out tasks: which element key was processed.
    pub(crate) fanout_key: Option<u64>,
    /// Which input slot the fan-out came from (to clear dirty on).
    pub(crate) fanout_slot: Option<usize>,
}
// ---------------------------------------------------------------------------
// TaskGraph
// ---------------------------------------------------------------------------
pub(crate) struct TaskGraph {
    pub(crate) tasks: Vec<ReadyTask>,
}
/// A fully pre-loaded task, ready for the scheduler to execute.
pub(crate) struct ReadyTask {
    pub(crate) id:           NodeId,
    pub(crate) erased:       ErasedTransform,
    /// Pre-loaded inputs indexed by slot.
    pub(crate) single_inputs:     Vec<Option<Box<dyn std::any::Any + Send + Sync>>>,
    pub(crate) collection_inputs: Vec<Option<crate::transform::ErasedCollection>>,
    pub(crate) registry:     Arc<ValueTypeRegistry>,
    pub(crate) fanout_slot:  Option<usize>,
    pub(crate) fanout_key:   Option<u64>,
    pub(crate) dependencies: Vec<NodeId>,
}
impl ReadyTask {
    pub(crate) async fn execute(self) -> TaskResult {
        let schema   = Arc::clone(&self.erased.schema);
        let dispatch = Arc::clone(&self.erased.dispatch);
        let mut ctx  = TransformContext::new(
            Arc::clone(&schema),
            Arc::clone(&dispatch),
            Arc::clone(&self.registry),
        );
        ctx.single_inputs     = self.single_inputs;
        ctx.collection_inputs = self.collection_inputs;
        if let Err(e) = self.erased.apply(&mut ctx).await {
            return TaskResult {
                id:          self.id,
                outputs:     vec![],
                fanout_key:  self.fanout_key,
                fanout_slot: self.fanout_slot,
                error:       Some(e),
            };
        }
        let outputs: Vec<Option<OutputSlot>> = ctx.outputs.into_iter().map(|opt| {
            opt.map(|o| match o {
                ContextOutput::Single(v, h)      => OutputSlot::Single(v, h),
                ContextOutput::Collection(pairs) => OutputSlot::Collection(pairs),
            })
        }).collect();
        TaskResult {
            id:          self.id,
            outputs,
            fanout_key:  self.fanout_key,
            fanout_slot: self.fanout_slot,
            error:       None,
        }
    }
}
impl TaskGraph {
    pub(crate) fn is_empty(&self) -> bool { self.tasks.is_empty() }
    pub(crate) fn len(&self) -> usize { self.tasks.len() }
}
// ---------------------------------------------------------------------------
// ExecutionContext
// ---------------------------------------------------------------------------
pub(crate) struct ExecutionContext {
    pub(crate) workstate: WorkState,
    pub(crate) loader:    Arc<Loader>,
    pub(crate) registry:  Arc<ValueTypeRegistry>,
}
impl ExecutionContext {
    pub(crate) fn new(loader: Arc<Loader>, registry: Arc<ValueTypeRegistry>) -> Self {
        Self { workstate: WorkState::new(), loader, registry }
    }
    // -----------------------------------------------------------------------
    // WorkState delegation helpers (used by Engine)
    // -----------------------------------------------------------------------
    pub(crate) fn mark_dirty(&mut self, id: NodeId) { self.workstate.mark_dirty(id); }
    pub(crate) fn mark_clean(&mut self, id: NodeId) { self.workstate.mark_clean(id); }
    pub(crate) fn is_dirty(&self, id: NodeId)       -> bool { self.workstate.is_dirty(id) }
    pub(crate) fn init_edge(&mut self, eid: EdgeId, is_collection: bool) {
        self.workstate.init_edge(eid, is_collection);
    }
    pub(crate) fn set_input_value(
        &mut self,
        topology:  &Topology,
        io_node:   NodeId,
        value:     Value,
        hash:      ValueHash,
    ) -> bool {
        let adj = match topology.io_adjacency(io_node) { Some(a) => a, None => return false };
        let mut any_changed = false;
        for &eid in &adj.outgoing {
            if self.workstate.write_single(eid, value.clone(), hash) {
                any_changed = true;
                // Dirty downstream transforms.
                if let Some(edge) = topology.edge(eid) {
                    if let Endpoint::TransformInput { transform, .. } = edge.to {
                        self.workstate.mark_dirty(transform);
                    }
                }
            }
        }
        any_changed
    }
    pub(crate) fn preload_input_value(
        &mut self, topology: &Topology, io_node: NodeId, value: Value, hash: ValueHash,
    ) {
        let adj = match topology.io_adjacency(io_node) { Some(a) => a, None => return };
        for &eid in &adj.outgoing {
            self.workstate.preload_single(eid, value.clone(), hash);
        }
    }
    pub(crate) fn peek_output(&self, topology: &Topology, io_node: NodeId) -> Option<(Value, ValueHash)> {
        let adj = topology.io_adjacency(io_node)?;
        let eid = adj.incoming?;
        self.workstate.read_single(eid)
    }
    pub(crate) fn clear_values_and_mark_all_dirty(&mut self, topology: &Topology) {
        self.workstate.clear_all_values();
        self.workstate.mark_all_dirty(topology.all_transform_ids().collect::<Vec<_>>().iter());
    }
    pub(crate) fn try_mark_all_clean_on_warm_start(&mut self, topology: &Topology) {
        // Only mark clean if at least one output has a value.
        let has_output = topology.io_nodes.values().any(|adj| {
            adj.incoming.map(|eid| self.workstate.read_single(eid).is_some()).unwrap_or(false)
        });
        if has_output {
            self.workstate.mark_all_clean(topology.all_transform_ids().collect::<Vec<_>>().iter());
        }
    }
    // -----------------------------------------------------------------------
    // Removal propagation (called before build_task_graph each cycle)
    // -----------------------------------------------------------------------

    /// For each fan-out transform node in topology order: if any input collection
    /// has fewer elements than the corresponding output collection, remove the
    /// stale output elements and mark downstream transforms dirty.
    ///
    /// Must be called BEFORE `build_task_graph` so that the task graph is built
    /// with an accurate (post-removal) dirty set.
    pub(crate) fn propagate_removals(&mut self, topology: &Topology) {
        for &tid in topology.topo_order() {
            let meta = match topology.transform_meta(tid) { Some(m) => m, None => continue };
            let schema = meta.layout();
            let n_in = schema.inputs.len();

            for slot in 0..n_in {
                // Only handle fan-out crossing: Single slot fed by a collection edge.
                if schema.inputs[slot].is_col { continue; }
                if !topology.input_slot_is_collection(tid, slot) { continue; }

                // Collect current input keys for this fan-out slot.
                let input_keys: std::collections::HashSet<u64> = meta.input_edges
                    .get(slot)
                    .map(|eids| eids.iter().flat_map(|&eid| {
                        self.workstate.read_collection(eid).iter().map(|e| e.key)
                    }).collect())
                    .unwrap_or_default();

                // For each output slot, remove elements not in input_keys.
                let n_out = schema.outputs.len();
                for out_slot in 0..n_out {
                    let removed_keys: std::collections::HashSet<u64> = meta.output_edges
                        .get(out_slot)
                        .map(|eids| eids.iter().flat_map(|&eid| {
                            self.workstate.collection_keys(eid)
                                .into_iter()
                                .filter(|k| !input_keys.contains(k))
                        }).collect())
                        .unwrap_or_default();

                    if !removed_keys.is_empty() {
                        if let Some(eids) = meta.output_edges.get(out_slot) {
                            for &eid in eids {
                                if self.workstate.remove_elements(eid, &removed_keys) {
                                    // Dirty downstream.
                                    if let Some(edge) = topology.edge(eid) {
                                        self.dirty_downstream(topology, edge.to);
                                    }
                                }
                            }
                        }
                        // Also mark this transform dirty if it wasn't already,
                        // so the collection gather (CollectTransform) re-runs.
                        self.workstate.mark_dirty(tid);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // TaskGraph construction
    // -----------------------------------------------------------------------
    /// Build a TaskGraph for the current dirty set.
    /// Reads input values from WorkState; produces self-contained tasks.
    pub(crate) fn build_task_graph(&self, topology: &Topology) -> TaskGraph {
        let dirty: Vec<NodeId> = topology.topo_order().iter()
            .copied()
            .filter(|&id| self.workstate.is_dirty(id))
            .collect();
        let dirty_set: HashSet<NodeId> = dirty.iter().copied().collect();
        let mut tasks = Vec::new();
        for id in dirty {
            let meta = match topology.transform_meta(id) { Some(m) => m, None => continue };
            let schema   = meta.layout();
            let dispatch = meta.dispatch();
            let n_in  = schema.inputs.len();
            // Detect fan-out: any Single-slot input slot fed by a collection edge.
            let fanout_slot = (0..n_in).find(|&slot| {
                !schema.inputs[slot].is_col && topology.input_slot_is_collection(id, slot)
            });
            if let Some(fslot) = fanout_slot {
                // Fan-out mode: produce one task per dirty element.
                let dirty_elements = self.collect_dirty_elements_for_slot(topology, id, fslot);
                // Also detect removed elements.
                let all_elements   = self.collect_all_elements_for_slot(topology, id, fslot);
                let current_keys: HashSet<u64> = all_elements.iter().map(|e| e.key).collect();
                // Collect the existing output keys to detect removals.
                for &out_slot in &(0..schema.outputs.len()).collect::<Vec<_>>() {
                    let removed: HashSet<u64> = topology
                        .transform_meta(id).unwrap()
                        .output_edges.get(out_slot)
                        .map(|eids| {
                            eids.iter().flat_map(|&eid| {
                                self.workstate.collection_keys(eid)
                                    .into_iter()
                                    .filter(|k| !current_keys.contains(k))
                            }).collect()
                        })
                        .unwrap_or_default();
                    let _ = removed; // Removal will be applied in apply_results.
                }
                let deps = self.compute_dependencies(id, &dirty_set, topology);
                for elem in dirty_elements {
                    let fanout_downcast = dispatch.inputs[fslot].downcast;
                    let boxed = match fanout_downcast(&elem.value, &self.registry) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let mut single_inputs: Vec<Option<Box<dyn std::any::Any + Send + Sync>>> =
                        (0..n_in).map(|_| None).collect();
                    single_inputs[fslot] = Some(boxed);
                    // Fill other non-fanout single inputs.
                    let mut ok = true;
                    for (slot, sinfo) in schema.inputs.iter().enumerate() {
                        if slot == fslot || sinfo.is_col { continue; }
                        let val = self.read_single_for_slot(topology, id, slot);
                        match val {
                            Some((v, _)) => {
                                let dc = dispatch.inputs[slot].downcast;
                                match dc(&v, &self.registry) {
                                    Ok(b) => single_inputs[slot] = Some(b),
                                    Err(_) => { ok = false; break; }
                                }
                            }
                            None => { ok = false; break; }
                        }
                    }
                    if !ok { continue; }
                    tasks.push(ReadyTask {
                        id,
                        erased:            meta.erased.clone(),
                        single_inputs,
                        collection_inputs: (0..n_in).map(|_| None).collect(),
                        registry:          Arc::clone(&self.registry),
                        fanout_slot:       Some(fslot),
                        fanout_key:        Some(elem.key),
                        dependencies:      deps.clone(),
                    });
                }
            } else {
                // Normal (non-fanout) mode: one task.
                let mut single_inputs:     Vec<Option<Box<dyn std::any::Any + Send + Sync>>> =
                    (0..n_in).map(|_| None).collect();
                let mut collection_inputs: Vec<Option<crate::transform::ErasedCollection>> =
                    (0..n_in).map(|_| None).collect();
                let mut ok = true;
                for (slot, sinfo) in schema.inputs.iter().enumerate() {
                    if sinfo.is_col {
                        let elems = self.collect_all_elements_for_slot(topology, id, slot);
                        if let Some(build) = dispatch.inputs[slot].build_collection {
                            let dirty_keys: Vec<u64> = elems.iter().filter(|e| e.dirty).map(|e| e.key).collect();
                            let all_values: Vec<Value> = elems.iter().map(|e| e.value.clone()).collect();
                            collection_inputs[slot] = Some(build(all_values, dirty_keys, &self.registry));
                        }
                    } else {
                        match self.read_single_for_slot(topology, id, slot) {
                            Some((v, _)) => {
                                let dc = dispatch.inputs[slot].downcast;
                                match dc(&v, &self.registry) {
                                    Ok(b) => single_inputs[slot] = Some(b),
                                    Err(_) => { ok = false; break; }
                                }
                            }
                            None => { ok = false; break; }
                        }
                    }
                }
                if !ok { continue; }
                let deps = self.compute_dependencies(id, &dirty_set, topology);
                tasks.push(ReadyTask {
                    id,
                    erased:           meta.erased.clone(),
                    single_inputs,
                    collection_inputs,
                    registry:         Arc::clone(&self.registry),
                    fanout_slot:      None,
                    fanout_key:       None,
                    dependencies:     deps,
                });
            }
        }
        TaskGraph { tasks }
    }
    // -----------------------------------------------------------------------
    // Result application
    // -----------------------------------------------------------------------
    /// Apply completed task results back into WorkState, then
    /// write to storage via Loader.  Returns whether anything changed.
    pub(crate) async fn apply_results(
        &mut self,
        topology: &Topology,
        results:  Vec<TaskResult>,
    ) -> (usize, usize, Vec<(Uuid, TransformError)>) {
        let mut changed       = 0usize;
        let mut coll_changed  = 0usize;
        let mut errors: Vec<(Uuid, TransformError)> = vec![];
        // First pass: handle fan-out removals for each fan-out transform.
        let mut fanout_transforms_seen: HashSet<NodeId> = HashSet::new();
        for r in &results {
            if let Some(fslot) = r.fanout_slot {
                if fanout_transforms_seen.insert(r.id) {
                    // Collect current input keys for this transform's fan-out slot.
                    let meta = match topology.transform_meta(r.id) { Some(m) => m, None => continue };
                    let current_keys: HashSet<u64> = meta.input_edges.get(fslot)
                        .map(|eids| eids.iter().flat_map(|&eid| {
                            self.workstate.read_collection(eid)
                                .iter().map(|e| e.key)
                        }).collect())
                        .unwrap_or_default();
                    let n_out = meta.erased.schema.outputs.len();
                    for out_slot in 0..n_out {
                        let removed_keys: HashSet<u64> = meta.output_edges.get(out_slot)
                            .map(|eids| eids.iter().flat_map(|&eid| {
                                self.workstate.collection_keys(eid)
                                    .into_iter()
                                    .filter(|k| !current_keys.contains(k))
                            }).collect())
                            .unwrap_or_default();
                        if !removed_keys.is_empty() {
                            if let Some(eids) = meta.output_edges.get(out_slot) {
                                for &eid in eids {
                                    if self.workstate.remove_elements(eid, &removed_keys) {
                                        changed += 1;
                                        // Dirty downstream.
                                        if let Some(edge) = topology.edge(eid) {
                                            self.dirty_downstream(topology, edge.to);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // Second pass: apply outputs.
        for r in results {
            if let Some(e) = r.error {
                self.workstate.mark_error(r.id, e.clone());
                errors.push((r.id.as_uuid(), e));
                continue;
            }
            let meta = match topology.transform_meta(r.id) { Some(m) => m, None => continue };
            let mut this_changed = false;
            for (out_slot, opt_out) in r.outputs.into_iter().enumerate() {
                let Some(out) = opt_out else { continue };
                let eids = match meta.output_edges.get(out_slot) {
                    Some(ids) => ids.clone(),
                    None => continue,
                };
                match out {
                    OutputSlot::Single(v, h) => {
                        if let Some(fanout_key) = r.fanout_key {
                            // Fan-out result: upsert as collection element.
                            for &eid in &eids {
                                if self.workstate.upsert_element(eid, fanout_key, v.clone(), h) {
                                    this_changed = true;
                                    if let Some(edge) = topology.edge(eid) {
                                        self.dirty_downstream(topology, edge.to);
                                        if let Endpoint::Io(nid) = edge.to {
                                            let _ = self.loader.persist_element(nid, fanout_key, &v, h).await;
                                        }
                                    }
                                }
                            }
                        } else {
                            // Normal single output.
                            for &eid in &eids {
                                if self.workstate.write_single(eid, v.clone(), h) {
                                    this_changed = true;
                                    if let Some(edge) = topology.edge(eid) {
                                        self.dirty_downstream(topology, edge.to);
                                        // Also write to the IoNode's incoming edge so peek_output works.
                                        if let Endpoint::Io(nid) = edge.to {
                                            if let Some(io_adj) = topology.io_adjacency(nid) {
                                                if let Some(in_eid) = io_adj.incoming {
                                                    self.workstate.write_single(in_eid, v.clone(), h);
                                                }
                                            }
                                            let _ = self.loader.persist(nid, &v, h).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    OutputSlot::Collection(pairs) => {
                        let n = pairs.len();
                        let pairs_ref: Vec<(u64, Value, ValueHash)> = pairs;
                        for &eid in &eids {
                            if self.workstate.replace_collection(eid, &pairs_ref) {
                                this_changed = true;
                                coll_changed += n;
                                if let Some(edge) = topology.edge(eid) {
                                    self.dirty_downstream(topology, edge.to);
                                    if let Endpoint::Io(nid) = edge.to {
                                        for (key, v, h) in &pairs_ref {
                                            let _ = self.loader.persist_element(nid, *key, v, *h).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Clear fan-out slot dirty flags after processing.
            if let Some(fslot) = r.fanout_slot {
                if let Some(eids) = meta.input_edges.get(fslot) {
                    for &eid in eids { self.workstate.clear_dirty(eid); }
                }
            }
            if this_changed { changed += 1; }
            self.workstate.mark_clean(r.id);
        }
        (changed, coll_changed, errors)
    }
    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------
    fn dirty_downstream(&mut self, topology: &Topology, endpoint: Endpoint) {
        match endpoint {
            Endpoint::TransformInput { transform, .. } => {
                self.workstate.mark_dirty(transform);
            }
            Endpoint::Io(io_n) => {
                // Propagate through the IoNode to its downstream transforms.
                if let Some(adj) = topology.io_adjacency(io_n) {
                    for &eid in &adj.outgoing {
                        if let Some(edge) = topology.edge(eid) {
                            if let Endpoint::TransformInput { transform, .. } = edge.to {
                                self.workstate.mark_dirty(transform);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn read_single_for_slot(&self, topology: &Topology, tid: NodeId, slot: usize) -> Option<(Value, ValueHash)> {
        let meta = topology.transform_meta(tid)?;
        let eids = meta.input_edges.get(slot)?;
        for &eid in eids {
            // Try reading directly from this edge first.
            if let Some(v) = self.workstate.read_single(eid) { return Some(v); }
            // If this edge goes from an IoNode, follow the IoNode's incoming edge.
            if let Some(edge) = topology.edge(eid) {
                if let crate::topology::Endpoint::Io(io_n) = edge.from {
                    if let Some(io_adj) = topology.io_adjacency(io_n) {
                        if let Some(in_eid) = io_adj.incoming {
                            if let Some(v) = self.workstate.read_single(in_eid) { return Some(v); }
                        }
                    }
                }
            }
        }
        None
    }
    fn collect_all_elements_for_slot<'a>(&'a self, topology: &'a Topology, tid: NodeId, slot: usize) -> Vec<&'a CollectionElement> {
        let meta = match topology.transform_meta(tid) { Some(m) => m, None => return vec![] };
        let eids = match meta.input_edges.get(slot) { Some(ids) => ids, None => return vec![] };
        let mut all = vec![];
        for &eid in eids {
            all.extend(self.workstate.read_collection(eid));
        }
        all
    }
    fn collect_dirty_elements_for_slot<'a>(&'a self, topology: &'a Topology, tid: NodeId, slot: usize) -> Vec<&'a CollectionElement> {
        self.collect_all_elements_for_slot(topology, tid, slot)
            .into_iter()
            .filter(|e| e.dirty)
            .collect()
    }
    fn compute_dependencies(&self, tid: NodeId, dirty_set: &HashSet<NodeId>, topology: &Topology) -> Vec<NodeId> {
        let schema = match topology.transform_meta(tid) { Some(m) => m, None => return vec![] };
        let mut deps = vec![];
        for slot in 0..schema.input_edges.len() {
            for &src in topology.input_slot_sources(tid, slot).iter() {
                if dirty_set.contains(&src) { deps.push(src); }
            }
        }
        deps
    }
}
