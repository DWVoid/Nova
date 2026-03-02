//! Parallel recomputation engine for the bipartite incremental graph.
//!
//! ## Design: Wave-Based Parallel Execution over TransformNodes
//!
//! The dirty subgraph is processed in topological waves over `TransformNode`s.
//! Within a wave, all transforms are independent and run concurrently via
//! `tokio::spawn`.
//!
//! ## Design: Crossing-Kind Edge Handling
//!
//! The scheduler resolves `Collection→Single` (per-element) and
//! `Single→Collection` (insert-into-gather) connections transparently:
//!
//! - **`Collection→Single`**: the transform is invoked once per dirty element.
//! - **`Single→Collection`**: the single value is upserted into the collection
//!   edge's element list using the slot's registered sorter.
//!
//! ## Design: SCC Fixed-Point Loop
//!
//! Legal cycles (SCC groups with Collection back-edges) are processed with a
//! bounded iteration loop that re-runs dirty members until convergence
//! (no collection element changed) or the cycle limit is hit.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use futures::future::join_all;
use crate::collection::{CollectionDiff, ElementKey};
use crate::graph::{EdgeId, EdgePayload, Endpoint, Graph};
use crate::loader::LazyLoader;
use crate::node_id::NodeId;
use crate::registry::SorterFn;
use crate::transform::{SlotInput, SlotOutput, TransformError};
use crate::value::{Value, ValueHash, hash_bytes};

// ---------------------------------------------------------------------------
// UpdateReport
// ---------------------------------------------------------------------------

/// Summary of a single incremental update cycle.
#[derive(Debug, Default)]
pub struct UpdateReport {
    pub transforms_evaluated:         usize,
    pub transforms_changed:           usize,
    pub transforms_skipped:           usize,
    pub transforms_blocked:           usize,
    pub collection_elements_changed:  usize,
    pub cycles_iterated:              usize,
    pub errors:                       Vec<(NodeId, TransformError)>,
    pub cycle_limit_exceeded:         Vec<NodeId>,
}

impl UpdateReport {
    pub fn is_ok(&self) -> bool { self.errors.is_empty() && self.cycle_limit_exceeded.is_empty() }

    // Backward-compat aliases.
    #[deprecated(note = "use transforms_evaluated")]
    pub fn nodes_evaluated(&self) -> usize { self.transforms_evaluated }
    #[deprecated(note = "use transforms_changed")]
    pub fn nodes_changed(&self) -> usize { self.transforms_changed }
    #[deprecated(note = "use transforms_skipped")]
    pub fn nodes_skipped(&self) -> usize { self.transforms_skipped }
    #[deprecated(note = "use transforms_blocked")]
    pub fn nodes_blocked(&self) -> usize { self.transforms_blocked }
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// Parallel incremental recomputation engine.
#[derive(Clone)]
pub struct Scheduler {
    graph:        Arc<Graph>,
    loader:       Arc<LazyLoader>,
    cycle_limit:  u32,
    /// Registered sorter functions (key → comparator), shared from engine.
    sorters:      Arc<std::sync::RwLock<HashMap<String, SorterFn>>>,
}

impl Scheduler {
    pub fn new(graph: Arc<Graph>, loader: Arc<LazyLoader>) -> Self {
        Self {
            graph,
            loader,
            cycle_limit: 1000,
            sorters: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn with_cycle_limit(mut self, limit: u32) -> Self {
        self.cycle_limit = limit;
        self
    }

    pub fn set_sorters(&mut self, sorters: Arc<std::sync::RwLock<HashMap<String, SorterFn>>>) {
        self.sorters = sorters;
    }

    pub fn set_cycle_limit(&mut self, limit: u32) {
        self.cycle_limit = limit;
    }

    /// Run one incremental update cycle.
    pub async fn run_update(&self) -> UpdateReport {
        let topo = self.graph.dirty_transforms_topo();
        if topo.is_empty() { return UpdateReport::default(); }

        let waves = compute_waves(&topo, &self.graph);
        let mut report = UpdateReport::default();
        // in-memory prev_output cache: cleared at end of update()
        let mut prev_outputs: HashMap<NodeId, Vec<SlotOutput>> = HashMap::new();

        // -----------------------------------------------------------------------
        // Identify SCC members so we can give them fixed-point treatment.
        // -----------------------------------------------------------------------
        let scc_groups = self.graph.scc_groups();
        let _scc_member: HashSet<NodeId> = scc_groups.iter()
            .flat_map(|g| g.members.iter().copied())
            .collect();

        // -----------------------------------------------------------------------
        // Run the main wave pass (acyclic nodes + first pass of SCC nodes).
        // -----------------------------------------------------------------------
        for wave in &waves {
            let handles: Vec<_> = wave.iter().map(|&nid| {
                let graph   = Arc::clone(&self.graph);
                let loader  = Arc::clone(&self.loader);
                let sorters = Arc::clone(&self.sorters);
                let prev    = prev_outputs.get(&nid).cloned();
                tokio::spawn(async move {
                    evaluate_transform(nid, graph, loader, sorters, prev.as_deref()).await
                })
            }).collect();
            let results = join_all(handles).await;
            for (i, result) in results.into_iter().enumerate() {
                let nid = wave[i];
                match result {
                    Err(je) => eprintln!("[incremental] task panicked: {je}"),
                    Ok(outcome) => collect_outcome(outcome, nid, &mut report, &mut prev_outputs),
                }
            }
        }

        // -----------------------------------------------------------------------
        // Fixed-point loop for each SCC group.
        // -----------------------------------------------------------------------
        for scc in &scc_groups {
            let limit = self.cycle_limit;
            let mut iterations = 0u32;
            loop {
                // Check if any SCC member is still dirty.
                let dirty_members: Vec<NodeId> = scc.members.iter().copied()
                    .filter(|&id| self.graph.transform_status(id)
                        .map(|s| s.is_dirty()).unwrap_or(false))
                    .collect();
                if dirty_members.is_empty() { break; }
                if iterations >= limit {
                    report.cycle_limit_exceeded.extend(scc.members.iter().copied());
                    break;
                }
                iterations += 1;
                // Snapshot collection hashes before this iteration.
                let pre_hashes = snapshot_scc_collection_hashes(scc, &self.graph);

                // Run dirty SCC members sequentially (cycles have ordering constraints).
                let topo_scc = topo_sort_subset(&dirty_members, &self.graph);
                for &nid in &topo_scc {
                    let graph   = Arc::clone(&self.graph);
                    let loader  = Arc::clone(&self.loader);
                    let sorters = Arc::clone(&self.sorters);
                    let prev    = prev_outputs.get(&nid).cloned();
                    let outcome = evaluate_transform(nid, graph, loader, sorters, prev.as_deref()).await;
                    collect_outcome(outcome, nid, &mut report, &mut prev_outputs);
                }
                // Check convergence: did any collection in the SCC change?
                let post_hashes = snapshot_scc_collection_hashes(scc, &self.graph);
                if pre_hashes == post_hashes { break; }
            }
            report.cycles_iterated += iterations as usize;
        }

        report
    }
}

// ---------------------------------------------------------------------------
// Helper: collect outcome into report
// ---------------------------------------------------------------------------

fn collect_outcome(
    outcome: TransformOutcome,
    nid: NodeId,
    report: &mut UpdateReport,
    prev_outputs: &mut HashMap<NodeId, Vec<SlotOutput>>,
) {
    match outcome {
        TransformOutcome::Changed { outputs, elements_changed } => {
            report.transforms_evaluated += 1;
            report.transforms_changed += 1;
            report.collection_elements_changed += elements_changed;
            prev_outputs.insert(nid, outputs);
        }
        TransformOutcome::Unchanged { outputs } => {
            report.transforms_evaluated += 1;
            report.transforms_skipped += 1;
            prev_outputs.insert(nid, outputs);
        }
        TransformOutcome::Blocked => {
            report.transforms_blocked += 1;
        }
        TransformOutcome::Error(err) => {
            report.transforms_evaluated += 1;
            report.errors.push((nid, err));
        }
    }
}

// ---------------------------------------------------------------------------
// Wave decomposition
// ---------------------------------------------------------------------------

fn compute_waves(topo: &[NodeId], graph: &Graph) -> Vec<Vec<NodeId>> {
    let mut wave_of: HashMap<NodeId, usize> = HashMap::new();
    for &nid in topo {
        // A transform is in wave max(wave of its dirty upstream transforms) + 1.
        if let Some(tn) = graph.get_transform_node(nid) {
            let max_pred = tn.input_edges.iter().flat_map(|slot| slot.iter()).filter_map(|&eid| {
                let edge = graph.get_edge(eid)?;
                let upstream = upstream_transform_of(&edge.from, graph);
                upstream.into_iter().filter_map(|u| wave_of.get(&u).copied()).max()
            }).max();
            wave_of.insert(nid, max_pred.map(|w| w + 1).unwrap_or(0));
        } else {
            wave_of.insert(nid, 0);
        }
    }
    let max_wave = wave_of.values().copied().max().unwrap_or(0);
    let mut waves: Vec<Vec<NodeId>> = vec![vec![]; max_wave + 1];
    for &nid in topo {
        waves[wave_of[&nid]].push(nid);
    }
    waves.retain(|w| !w.is_empty());
    waves
}

fn upstream_transform_of(from: &Endpoint, graph: &Graph) -> Vec<NodeId> {
    match from {
        Endpoint::TransformOutput { transform, .. } => vec![*transform],
        Endpoint::Io(io_id) => {
            if let Some(node) = graph.get_io_node(*io_id) {
                if let Some(eid) = node.incoming {
                    if let Some(edge) = graph.get_edge(eid) {
                        return upstream_transform_of(&edge.from.clone(), graph);
                    }
                }
            }
            vec![]
        }
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// SCC helpers
// ---------------------------------------------------------------------------

fn snapshot_scc_collection_hashes(scc: &crate::cycle::SccGroup, graph: &Graph) -> Vec<(EdgeId, u64)> {
    let mut hashes = Vec::new();
    for &tid in &scc.members {
        if let Some(tn) = graph.get_transform_node(tid) {
            for slot_edges in &tn.input_edges {
                for &eid in slot_edges {
                    if let Some(edge) = graph.get_edge(eid) {
                        if let EdgePayload::Collection(c) = &edge.payload {
                            hashes.push((eid, c.full_hash));
                        }
                    }
                }
            }
        }
    }
    hashes.sort_by_key(|(eid, _)| eid.0);
    hashes
}

fn topo_sort_subset(nodes: &[NodeId], graph: &Graph) -> Vec<NodeId> {
    // Simple Kahn over the given subset.
    let set: HashSet<NodeId> = nodes.iter().copied().collect();
    let mut in_degree: HashMap<NodeId, usize> = nodes.iter().map(|&n| (n, 0)).collect();
    let mut adj: HashMap<NodeId, Vec<NodeId>> = nodes.iter().map(|&n| (n, vec![])).collect();
    for &nid in nodes {
        if let Some(tn) = graph.get_transform_node(nid) {
            for slot_edges in &tn.input_edges {
                for &eid in slot_edges {
                    if let Some(edge) = graph.get_edge(eid) {
                        for u in upstream_transform_of(&edge.from, graph) {
                            if set.contains(&u) {
                                *in_degree.entry(nid).or_default() += 1;
                                adj.entry(u).or_default().push(nid);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut queue: std::collections::VecDeque<NodeId> = in_degree.iter()
        .filter(|(_, d)| **d == 0).map(|(n, _)| *n).collect();
    let mut result = Vec::new();
    while let Some(n) = queue.pop_front() {
        result.push(n);
        if let Some(ns) = adj.get(&n) {
            for &next in ns {
                let d = in_degree.entry(next).or_default();
                *d = d.saturating_sub(1);
                if *d == 0 { queue.push_back(next); }
            }
        }
    }
    // Append any remaining (SCC back-edges may prevent full drain).
    for &n in nodes {
        if !result.contains(&n) { result.push(n); }
    }
    result
}

// ---------------------------------------------------------------------------
// Per-transform evaluation
// ---------------------------------------------------------------------------

enum TransformOutcome {
    Changed { outputs: Vec<SlotOutput>, elements_changed: usize },
    Unchanged { outputs: Vec<SlotOutput> },
    Blocked,
    Error(TransformError),
}

async fn evaluate_transform(
    tid: NodeId,
    graph: Arc<Graph>,
    loader: Arc<LazyLoader>,
    sorters: Arc<std::sync::RwLock<HashMap<String, SorterFn>>>,
    prev_output: Option<&[SlotOutput]>,
) -> TransformOutcome {
    let (transform, schema, input_edges, output_edges) = {
        let tn = match graph.get_transform_node(tid) {
            Some(t) => t,
            None => return TransformOutcome::Error(
                TransformError::new(format!("TransformNode {tid} not found"))),
        };
        (tn.transform.clone(), tn.transform.schema().clone(),
         tn.input_edges.clone(), tn.output_edges.clone())
    };

    // -----------------------------------------------------------------------
    // 1. Assemble SlotInputs
    // -----------------------------------------------------------------------
    let mut slot_inputs: Vec<SlotInput> = Vec::with_capacity(schema.inputs.len().max(input_edges.len()));
    if schema.inputs.is_empty() && !input_edges.is_empty() {
        // Dynamic-count shim (ManyToOne / ManyToMany): collect all edges as Singles.
        for slot_edges in &input_edges {
            for &eid in slot_edges {
                let value = match load_single_edge_value(eid, &graph, &loader).await {
                    Ok(v) => v,
                    Err(e) => {
                        if is_upstream_dirty_and_valueless(eid, &graph) {
                            return TransformOutcome::Blocked;
                        }
                        return TransformOutcome::Error(e);
                    }
                };
                slot_inputs.push(SlotInput::Single(value));
            }
        }
    } else {
        for (slot_idx, slot_desc) in schema.inputs.iter().enumerate() {
        let slot_edges = &input_edges[slot_idx];
        match &slot_desc.kind {
            crate::slot::SlotKind::Single => {
                if slot_edges.is_empty() {
                    return TransformOutcome::Error(TransformError::new(
                        format!("transform {tid} slot {slot_idx} has no incoming edges")));
                }
                let eid = slot_edges[0];
                let value = match load_single_edge_value(eid, &graph, &loader).await {
                    Ok(v) => v,
                    Err(e) => {
                        if is_upstream_dirty_and_valueless(eid, &graph) {
                            return TransformOutcome::Blocked;
                        }
                        return TransformOutcome::Error(e);
                    }
                };
                slot_inputs.push(SlotInput::Single(value));
            }
            crate::slot::SlotKind::Collection { .. } => {
                let (elements, diff) = gather_collection_slot(slot_edges, &graph, &loader).await;
                slot_inputs.push(SlotInput::Collection { elements, diff });
            }
        }
    }
    }

    // -----------------------------------------------------------------------
    // 2. Run the transform.
    // -----------------------------------------------------------------------
    let outputs = match transform.apply(&slot_inputs, prev_output).await {
        Ok(v) => v,
        Err(e) => {
            let _ = graph.store_transform_error(tid, e.clone());
            return TransformOutcome::Error(e);
        }
    };

    if outputs.len() != schema.outputs.len() && schema.outputs.len() > 0 {
        let err = TransformError::new(format!(
            "transform {tid} produced {} outputs but schema has {} output slots",
            outputs.len(), schema.outputs.len()
        ));
        let _ = graph.store_transform_error(tid, err.clone());
        return TransformOutcome::Error(err);
    }

    // -----------------------------------------------------------------------
    // 3. Store outputs on edges and propagate dirty if changed.
    // -----------------------------------------------------------------------
    let mut any_changed = false;
    let mut elements_changed = 0usize;

    // Handle dynamic-output transforms (schema.outputs.len() == 0 but output_edges has slots).
    let effective_outputs: Vec<(&SlotOutput, Option<&crate::slot::SlotDescriptor>)> = if schema.outputs.is_empty() {
        outputs.iter().enumerate().map(|(i, o)| {
            (o, None)
        }).collect()
    } else {
        outputs.iter().zip(schema.outputs.iter().map(Some)).collect()
    };

    for (slot_idx, (slot_output, slot_desc_opt)) in effective_outputs.iter().enumerate() {
        let empty = vec![];
        let slot_edges = output_edges.get(slot_idx).unwrap_or(&empty);
        match slot_output {
            SlotOutput::Single(value) => {
                let new_hash = hash_bytes(&loader.registry().serialize_value(value));
                for &eid in slot_edges {
                    let prev_hash = graph.edge_hash(eid);
                    if prev_hash == Some(new_hash) {
                        let _ = graph.store_single_value(eid, value.clone(), new_hash);
                    } else {
                        any_changed = true;
                        let _ = graph.store_single_value(eid, value.clone(), new_hash);
                        if let Some(edge) = graph.get_edge(eid) {
                            if let Endpoint::Io(io_id) = edge.to {
                                loader.cache_value(io_id, value.clone());
                            }
                        }
                        propagate_dirty_after_edge(&graph, eid);
                    }
                }
            }
            SlotOutput::Collection(new_pairs) => {
                let sorter_key = slot_desc_opt
                    .and_then(|d| if let crate::slot::SlotKind::Collection { sorter_key } = &d.kind { Some(sorter_key.as_str()) } else { None })
                    .unwrap_or("");
                let sorters_guard = sorters.read().unwrap();
                let sorter: Option<&SorterFn> = sorters_guard.get(sorter_key);
                let default_sorter: SorterFn = Arc::new(|_, _| std::cmp::Ordering::Equal);
                let sorter_ref: &SorterFn = sorter.unwrap_or(&default_sorter);
                let new_elements: Vec<(ElementKey, Value, ValueHash)> = new_pairs.iter().map(|(key, val)| {
                    let h = hash_bytes(&loader.registry().serialize_value(val));
                    (*key, val.clone(), h)
                }).collect();
                for &eid in slot_edges {
                    let diff = match graph.store_collection_diff(eid, new_elements.clone(), sorter_ref.as_ref()) {
                        Ok(d) => d,
                        Err(e) => {
                            let err = TransformError::new(e.message);
                            let _ = graph.store_transform_error(tid, err.clone());
                            return TransformOutcome::Error(err);
                        }
                    };
                    if !diff.is_empty() {
                        elements_changed += diff.added.len() + diff.removed.len() + diff.changed.len();
                        any_changed = true;
                        propagate_dirty_after_edge(&graph, eid);
                    }
                }
            }
        }
    }

    graph.mark_transform_clean(tid);

    if any_changed {
        TransformOutcome::Changed { outputs: outputs.clone(), elements_changed }
    } else {
        TransformOutcome::Unchanged { outputs: outputs.clone() }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn load_single_edge_value(
    eid: EdgeId,
    graph: &Graph,
    loader: &LazyLoader,
) -> Result<Value, TransformError> {
    // First try the edge's own cached value.
    if let Some((v, _)) = graph.peek_single_value(eid) {
        return Ok(v);
    }
    // The source might be an IoNode that is itself written by another edge.
    // Walk upstream: if source is an IoNode, check its incoming edge for a value.
    let src_io_id = {
        let edge = graph.get_edge(eid)
            .ok_or_else(|| TransformError::new(format!("edge {eid:?} not found")))?;
        match edge.from {
            Endpoint::Io(id) => Some(id),
            _ => None,
        }
    };
    if let Some(io_id) = src_io_id {
        // Check incoming edge of this IoNode (which may hold the computed value).
        if let Some((v, _)) = graph.peek_io_value(io_id) {
            return Ok(v);
        }
        // Try loader cache keyed by the IoNode's ID.
        match loader.get(io_id).await {
            Ok(Some(v)) => return Ok(v),
            _ => {}
        }
    }
    Err(TransformError::new(format!("no value on edge {eid:?}")))
}

fn is_upstream_dirty_and_valueless(eid: EdgeId, graph: &Graph) -> bool {
    if let Some(edge) = graph.get_edge(eid) {
        match edge.from {
            Endpoint::TransformOutput { transform, .. } => {
                if let Some(status) = graph.transform_status(transform) {
                    return status.is_dirty() || status.is_error();
                }
            }
            Endpoint::Io(io_id) => {
                // IoNode with no value: check if its incoming transform errored.
                if let Some((_, _)) = graph.peek_io_value(io_id) {
                    // Has a value — not blocked.
                    return false;
                }
                // No value. Check if the upstream transform errored.
                if let Some(node) = graph.get_io_node(io_id) {
                    if let Some(incoming_eid) = node.incoming {
                        drop(node);
                        return is_upstream_dirty_and_valueless(incoming_eid, graph);
                    }
                }
                // IoNode has no incoming edge and no value → input not set yet.
                return true;
            }
            _ => {}
        }
    }
    false
}

async fn gather_collection_slot(
    slot_edges: &[EdgeId],
    graph: &Graph,
    _loader: &LazyLoader,
) -> (Vec<Value>, CollectionDiff) {
    let mut elements: Vec<Value> = Vec::new();
    let mut diff = CollectionDiff::default();
    for &eid in slot_edges {
        if let Some(edge) = graph.get_edge(eid) {
            match &edge.payload {
                EdgePayload::Collection(c) => {
                    for el in &c.elements {
                        elements.push(el.value.clone());
                    }
                    // Accumulate diffs (simplified: mark all dirty elements as changed).
                    for el in c.elements.iter().filter(|e| e.dirty) {
                        diff.changed.push((el.key, el.value.clone(), el.value.clone()));
                    }
                }
                EdgePayload::Single(s) => {
                    // Single→Collection crossing: treat as one-element collection.
                    if let Some(v) = &s.value {
                        elements.push(v.clone());
                        if s.dirty {
                            diff.added.push((s.value_hash.unwrap_or(0), v.clone()));
                        }
                    }
                }
            }
        }
    }
    (elements, diff)
}

fn propagate_dirty_after_edge(graph: &Graph, eid: EdgeId) {
    if let Some(edge) = graph.get_edge(eid) {
        let to = edge.to.clone();
        drop(edge);
        match to {
            Endpoint::TransformInput { transform, .. } => {
                graph.mark_transform_dirty(transform);
            }
            Endpoint::Io(io_id) => {
                graph.propagate_dirty_from_io(io_id);
            }
            _ => {}
        }
    }
}