//! Mutable graph state + all topology-aware algorithms.
//!
//! `WorkState` is the single owner of all runtime-mutable data:
//! - per-node status (Dirty / Clean / Error)
//! - per-edge values (single values and collection elements)
//!
//! It also owns every algorithm that needs to *mutate* state with reference to
//! the graph topology:
//! - input writing with downstream dirty-propagation
//! - fan-out removal propagation
//! - incremental update pass (via [`Scheduler`])
//! - serialisation/deserialisation for warm-start ([`StateSnapshot`])
//!
//! ## Concurrency model
//!
//! During `run_pass` the inner state is wrapped in `Arc<RwLock<WorkStateInner>>`
//! so that the async transform closures can push results back without holding the
//! whole-engine lock.  Outside of a pass the caller holds the `Engine` mutex and
//! accesses `WorkState` exclusively.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use crossbeam_queue::SegQueue;
use serde::{Serialize, Deserialize};
use uuid::Uuid;

use crate::loader::Loader;
use crate::node_id::NodeId;
use crate::scheduler::{Scheduler, TaskId};
use crate::topology::{EdgeId, Endpoint, Topology};
use crate::transform::{ErasedTransform, TransformContext, TransformError, ContextOutput};
use crate::value::{Value, ValueHash, ValueTypeRegistry};

// ---------------------------------------------------------------------------
// NodeStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum NodeStatus {
    Dirty,
    Clean,
    Error(TransformError),
}

impl NodeStatus {
    pub(crate) fn is_dirty(&self) -> bool { matches!(self, NodeStatus::Dirty) }
}

// ---------------------------------------------------------------------------
// EdgeValue / CollectionElement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum EdgeValue {
    Single     { value: Option<Value>, hash: Option<ValueHash>, dirty: bool },
    Collection(Vec<CollectionElement>),
}

impl EdgeValue {
    pub(crate) fn single()     -> Self { Self::Single { value: None, hash: None, dirty: false } }
    pub(crate) fn collection() -> Self { Self::Collection(vec![]) }
}

#[derive(Debug, Clone)]
pub(crate) struct CollectionElement {
    pub(crate) key:   u64,
    pub(crate) value: Value,
    pub(crate) hash:  ValueHash,
    pub(crate) dirty: bool,
}

// ---------------------------------------------------------------------------
// StateSnapshot — serialisable warm-start image
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StateSnapshot {
    pub(crate) node_status:  HashMap<NodeId, bool>,       // true = dirty
    pub(crate) edge_values:  HashMap<EdgeId, SnapshotEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum SnapshotEdge {
    Single {
        type_key:    String,
        value_bytes: Vec<u8>,
        hash:        u64,
        dirty:       bool,
    },
    Collection(Vec<SnapshotElement>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SnapshotElement {
    pub(crate) key:         u64,
    pub(crate) type_key:    String,
    pub(crate) value_bytes: Vec<u8>,
    pub(crate) hash:        u64,
    pub(crate) dirty:       bool,
}

// ---------------------------------------------------------------------------
// OutputSlot / TaskOutput
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum OutputSlot {
    Single(Value, ValueHash),
    Collection(Vec<(u64, Value, ValueHash)>),
}

/// Result produced by one transform execution.  Internal to workstate.
pub(crate) struct TaskOutput {
    pub(crate) node_id:    NodeId,
    pub(crate) outputs:    Vec<Option<OutputSlot>>,
    pub(crate) error:      Option<TransformError>,
    pub(crate) fanout_key: Option<u64>,
    pub(crate) fanout_slot:Option<usize>,
}

// ---------------------------------------------------------------------------
// PassResult
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(crate) struct PassResult {
    pub(crate) changed:              usize,   // edges changed (internal)
    pub(crate) coll_changed:         usize,
    pub(crate) transforms_evaluated: usize,   // transforms that ran (dirty set size)
    pub(crate) transforms_changed:   usize,   // transforms that produced a change
    pub(crate) errors:               Vec<(Uuid, TransformError)>,
    pub(crate) nothing_happened:     bool,
}

// ---------------------------------------------------------------------------
// WorkStateInner — the actual mutable data
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(crate) struct WorkStateInner {
    pub(crate) node_status: HashMap<NodeId, NodeStatus>,
    pub(crate) edge_values: HashMap<EdgeId, EdgeValue>,
}

impl WorkStateInner {
    // -----------------------------------------------------------------------
    // Basic edge / node accessors
    // -----------------------------------------------------------------------

    pub(crate) fn mark_dirty(&mut self, id: NodeId) {
        self.node_status.insert(id, NodeStatus::Dirty);
    }
    pub(crate) fn mark_clean(&mut self, id: NodeId) {
        self.node_status.insert(id, NodeStatus::Clean);
    }
    pub(crate) fn mark_error(&mut self, id: NodeId, err: TransformError) {
        self.node_status.insert(id, NodeStatus::Error(err));
    }
    pub(crate) fn is_dirty(&self, id: NodeId) -> bool {
        self.node_status.get(&id).map(|s| s.is_dirty()).unwrap_or(true)
    }

    pub(crate) fn init_edge(&mut self, eid: EdgeId, is_collection: bool) {
        let v = if is_collection { EdgeValue::collection() } else { EdgeValue::single() };
        self.edge_values.entry(eid).or_insert(v);
    }

    pub(crate) fn read_single(&self, eid: EdgeId) -> Option<(Value, ValueHash)> {
        match self.edge_values.get(&eid)? {
            EdgeValue::Single { value: Some(v), hash: Some(h), .. } => Some((v.clone(), *h)),
            _ => None,
        }
    }

    pub(crate) fn read_collection(&self, eid: EdgeId) -> &[CollectionElement] {
        match self.edge_values.get(&eid) {
            Some(EdgeValue::Collection(v)) => v,
            _ => &[],
        }
    }

    pub(crate) fn collection_keys(&self, eid: EdgeId) -> Vec<u64> {
        match self.edge_values.get(&eid) {
            Some(EdgeValue::Collection(v)) => v.iter().map(|e| e.key).collect(),
            _ => vec![],
        }
    }

    pub(crate) fn write_single(&mut self, eid: EdgeId, value: Value, hash: ValueHash) -> bool {
        match self.edge_values.get_mut(&eid) {
            Some(EdgeValue::Single { value: v, hash: h, dirty: d }) => {
                if h.map(|old| old != hash).unwrap_or(true) {
                    *v = Some(value); *h = Some(hash); *d = true; true
                } else { false }
            }
            _ => {
                self.edge_values.insert(eid, EdgeValue::Single {
                    value: Some(value), hash: Some(hash), dirty: true,
                });
                true
            }
        }
    }

    pub(crate) fn preload_single(&mut self, eid: EdgeId, value: Value, hash: ValueHash) {
        self.edge_values.insert(eid, EdgeValue::Single {
            value: Some(value), hash: Some(hash), dirty: false,
        });
    }

    pub(crate) fn upsert_element(&mut self, eid: EdgeId, key: u64, value: Value, hash: ValueHash) -> bool {
        let elems = self.collection_mut(eid);
        if let Some(el) = elems.iter_mut().find(|e| e.key == key) {
            if el.hash != hash { el.value = value; el.hash = hash; el.dirty = true; true } else { false }
        } else {
            elems.push(CollectionElement { key, value, hash, dirty: true });
            true
        }
    }

    pub(crate) fn replace_collection(&mut self, eid: EdgeId, items: &[(u64, Value, ValueHash)]) -> bool {
        let new_keys: HashSet<u64> = items.iter().map(|(k, _, _)| *k).collect();
        let elems = self.collection_mut(eid);
        let before = elems.len();
        elems.retain(|el| new_keys.contains(&el.key));
        let mut changed = elems.len() < before;
        for (key, value, hash) in items {
            if let Some(el) = elems.iter_mut().find(|e| e.key == *key) {
                if el.hash != *hash {
                    el.value = value.clone(); el.hash = *hash; el.dirty = true; changed = true;
                }
            } else {
                elems.push(CollectionElement { key: *key, value: value.clone(), hash: *hash, dirty: true });
                changed = true;
            }
        }
        changed
    }

    pub(crate) fn remove_elements(&mut self, eid: EdgeId, keys: &HashSet<u64>) -> bool {
        if let Some(EdgeValue::Collection(elems)) = self.edge_values.get_mut(&eid) {
            let before = elems.len();
            elems.retain(|el| !keys.contains(&el.key));
            return elems.len() < before;
        }
        false
    }

    pub(crate) fn clear_dirty_on_edge(&mut self, eid: EdgeId) {
        match self.edge_values.get_mut(&eid) {
            Some(EdgeValue::Single { dirty: d, .. }) => *d = false,
            Some(EdgeValue::Collection(elems)) => { for el in elems.iter_mut() { el.dirty = false; } }
            None => {}
        }
    }

    pub(crate) fn clear_all_values(&mut self) {
        for v in self.edge_values.values_mut() {
            match v {
                EdgeValue::Single { value, hash, dirty } => { *value = None; *hash = None; *dirty = false; }
                EdgeValue::Collection(elems) => elems.clear(),
            }
        }
    }

    fn collection_mut(&mut self, eid: EdgeId) -> &mut Vec<CollectionElement> {
        let entry = self.edge_values.entry(eid).or_insert_with(EdgeValue::collection);
        match entry { EdgeValue::Collection(v) => v, _ => panic!("collection_mut on non-collection edge") }
    }
}

// ---------------------------------------------------------------------------
// WorkState — the public face (wraps WorkStateInner in Arc<RwLock>)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct WorkState {
    pub(crate) inner: Arc<RwLock<WorkStateInner>>,
}

impl WorkState {
    pub(crate) fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(WorkStateInner::default())) }
    }

    // -----------------------------------------------------------------------
    // Topology initialisation
    // -----------------------------------------------------------------------

    pub(crate) fn init_from_topology(&self, topo: &Topology) {
        let mut ws = self.inner.write().unwrap();
        for (eid, edge) in &topo.edges {
            ws.init_edge(*eid, edge.is_collection);
        }
        for id in topo.transform_nodes.keys() {
            ws.mark_dirty(*id);
        }
    }

    // -----------------------------------------------------------------------
    // Input node writing
    // -----------------------------------------------------------------------

    pub(crate) fn set_input(
        &self, topo: &Topology, io_node: NodeId, value: Value, hash: ValueHash,
    ) -> bool {
        let adj = match topo.io_adjacency(io_node) { Some(a) => a, None => return false };
        let outgoing: Vec<(EdgeId, NodeId)> = adj.outgoing.iter().filter_map(|&eid| {
            topo.edge(eid).and_then(|e| {
                if let Endpoint::TransformInput { transform, .. } = e.to { Some((eid, transform)) } else { None }
            })
        }).collect();

        let mut ws = self.inner.write().unwrap();
        let mut any = false;
        for (eid, transform_id) in outgoing {
            if ws.write_single(eid, value.clone(), hash) {
                ws.mark_dirty(transform_id);
                any = true;
            }
        }
        any
    }

    pub(crate) fn preload_input(
        &self, topo: &Topology, io_node: NodeId, value: Value, hash: ValueHash,
    ) {
        let adj = match topo.io_adjacency(io_node) { Some(a) => a, None => return };
        let mut ws = self.inner.write().unwrap();
        for &eid in &adj.outgoing {
            ws.preload_single(eid, value.clone(), hash);
        }
    }

    // -----------------------------------------------------------------------
    // Output reading
    // -----------------------------------------------------------------------

    pub(crate) fn peek_output(
        &self, topo: &Topology, io_node: NodeId,
    ) -> Option<(Value, ValueHash)> {
        let adj = topo.io_adjacency(io_node)?;
        let eid = adj.incoming?;
        self.inner.read().unwrap().read_single(eid)
    }

    // -----------------------------------------------------------------------
    // Removal propagation (pre-pass before run_pass)
    // -----------------------------------------------------------------------

    pub(crate) fn propagate_removals(&self, topo: &Topology) {
        // Collect work to do under read lock, then apply under write lock.
        struct Removal { eid: EdgeId, keys: HashSet<u64>, downstream: Endpoint }

        let mut removals: Vec<Removal> = vec![];
        let mut extra_dirty: Vec<NodeId> = vec![];

        {
            let ws = self.inner.read().unwrap();
            for &tid in topo.topo_order() {
                let meta = match topo.transform_meta(tid) { Some(m) => m, None => continue };
                let schema = meta.layout();
                for slot in 0..schema.inputs.len() {
                    if schema.inputs[slot].is_col { continue; }
                    if !topo.input_slot_is_collection(tid, slot) { continue; }

                    let input_keys: HashSet<u64> = meta.input_edges.get(slot)
                        .map(|eids| eids.iter().flat_map(|&eid| {
                            ws.read_collection(eid).iter().map(|e| e.key)
                        }).collect())
                        .unwrap_or_default();

                    for out_slot in 0..schema.outputs.len() {
                        if let Some(out_eids) = meta.output_edges.get(out_slot) {
                            for &eid in out_eids {
                                let removed: HashSet<u64> = ws.collection_keys(eid)
                                    .into_iter().filter(|k| !input_keys.contains(k)).collect();
                                if !removed.is_empty() {
                                    if let Some(edge) = topo.edge(eid) {
                                        removals.push(Removal { eid, keys: removed, downstream: edge.to });
                                    }
                                    extra_dirty.push(tid);
                                }
                            }
                        }
                    }
                }
            }
        }

        if removals.is_empty() { return; }

        let mut ws = self.inner.write().unwrap();
        for r in removals {
            if ws.remove_elements(r.eid, &r.keys) {
                Self::dirty_downstream_inner(&mut ws, topo, r.downstream);
            }
        }
        for tid in extra_dirty { ws.mark_dirty(tid); }
    }

    // -----------------------------------------------------------------------
    // Run one incremental update pass via the Scheduler
    // -----------------------------------------------------------------------

    pub(crate) async fn run_pass(
        &self,
        topo:     &Topology,
        loader:   &Arc<Loader>,
        registry: &Arc<ValueTypeRegistry>,
        sched:    &Scheduler,
    ) -> PassResult {
        // Collect dirty transform IDs in topological order.
        let dirty: Vec<NodeId> = {
            let ws = self.inner.read().unwrap();
            topo.topo_order().iter()
                .copied()
                .filter(|&id| ws.is_dirty(id))
                .collect()
        };

        if dirty.is_empty() {
            return PassResult { nothing_happened: true, ..Default::default() };
        }

        let n_evaluated = dirty.len();
        let dirty_set: HashSet<NodeId> = dirty.iter().copied().collect();

        // Output sink: tasks push TaskOutput here without holding any lock.
        let sink: Arc<SegQueue<TaskOutput>> = Arc::new(SegQueue::new());

        // Map from NodeId → TaskId so later tasks can express dependencies.
        let mut node_task_id: HashMap<NodeId, TaskId> = HashMap::new();

        for id in dirty {
            let meta = match topo.transform_meta(id) { Some(m) => m, None => continue };
            let schema   = meta.layout();
            let dispatch = meta.dispatch();
            let n_in = schema.inputs.len();

            // Detect fan-out slot: a Single-typed slot fed by a collection edge.
            let fanout_slot = (0..n_in).find(|&slot| {
                !schema.inputs[slot].is_col && topo.input_slot_is_collection(id, slot)
            });

            // Compute upstream dirty deps for this node.
            let dep_task_ids: Vec<TaskId> = {
                let mut deps = vec![];
                for slot in 0..meta.input_edges.len() {
                    for &src in &topo.input_slot_sources(id, slot) {
                        if dirty_set.contains(&src) {
                            if let Some(&tid) = node_task_id.get(&src) {
                                deps.push(tid);
                            }
                        }
                    }
                }
                deps.dedup();
                deps
            };

            if let Some(fslot) = fanout_slot {
                // Fan-out: spawn one task per dirty element.
                let dirty_elems: Vec<(u64, Value)> = {
                    let ws = self.inner.read().unwrap();
                    let eids = meta.input_edges.get(fslot).cloned().unwrap_or_default();
                    eids.iter().flat_map(|&eid| {
                        ws.read_collection(eid).iter()
                            .filter(|e| e.dirty)
                            .map(|e| (e.key, e.value.clone()))
                            .collect::<Vec<_>>()
                    }).collect()
                };

                let mut last_tid: Option<TaskId> = None;
                for (elem_key, elem_val) in dirty_elems {
                    // Build other non-fanout inputs.
                    let other_inputs = self.read_other_single_inputs(topo, id, fslot, dispatch, registry);
                    let other_inputs = match other_inputs {
                        Some(v) => v,
                        None => continue,
                    };
                    let fanout_dc = dispatch.inputs[fslot].downcast;
                    let boxed_elem = match fanout_dc(&elem_val, registry) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let payload = TransformPayload {
                        node_id:          id,
                        erased:           meta.erased.clone(),
                        single_inputs:    {
                            let mut v: Vec<Option<Box<dyn std::any::Any + Send + Sync>>> =
                                (0..n_in).map(|_| None).collect();
                            v[fslot] = Some(boxed_elem);
                            for (slot, b) in other_inputs { v[slot] = Some(b); }
                            v
                        },
                        collection_inputs: (0..n_in).map(|_| None).collect(),
                        registry:          Arc::clone(registry),
                        fanout_key:        Some(elem_key),
                        fanout_slot:       Some(fslot),
                        sink:              Arc::clone(&sink),
                    };
                    let task_deps: Vec<TaskId> = if let Some(t) = last_tid {
                        let mut d = dep_task_ids.clone(); d.push(t); d
                    } else {
                        dep_task_ids.clone()
                    };
                    let tid = if task_deps.is_empty() {
                        sched.spawn(async move { payload.run().await; }).await
                    } else {
                        sched.schedule(&task_deps, async move { payload.run().await; }).await
                    };
                    last_tid = Some(tid);
                    node_task_id.insert(id, tid);
                }
            } else {
                // Normal mode: one task per transform.
                let inputs = self.read_all_inputs(topo, id, dispatch, registry);
                let (single_inputs, collection_inputs) = match inputs {
                    Some(v) => v,
                    None => continue,
                };
                let payload = TransformPayload {
                    node_id:          id,
                    erased:           meta.erased.clone(),
                    single_inputs,
                    collection_inputs,
                    registry:         Arc::clone(registry),
                    fanout_key:       None,
                    fanout_slot:      None,
                    sink:             Arc::clone(&sink),
                };
                let tid = if dep_task_ids.is_empty() {
                    sched.spawn(async move { payload.run().await; }).await
                } else {
                    sched.schedule(&dep_task_ids, async move { payload.run().await; }).await
                };
                node_task_id.insert(id, tid);
            }
        }

        // Wait for all tasks.
        sched.join().await;

        // Apply all outputs.
        let mut result = self.apply_task_outputs(topo, loader, &sink, &dirty_set).await;
        result.transforms_evaluated = n_evaluated;
        result
    }

    // -----------------------------------------------------------------------
    // Apply task outputs to WorkState
    // -----------------------------------------------------------------------

    async fn apply_task_outputs(
        &self,
        topo:       &Topology,
        loader:     &Arc<Loader>,
        sink:       &Arc<SegQueue<TaskOutput>>,
        _dirty_set: &HashSet<NodeId>,
    ) -> PassResult {
        // Collect all outputs from the sink.
        let outputs: Vec<TaskOutput> = {
            let mut v = vec![];
            while let Some(o) = sink.pop() { v.push(o); }
            v
        };

        // Deferred async persistence work — collected during sync phase.
        enum PersistOp {
            Single  { nid: NodeId, value: Value, hash: ValueHash },
            Element { nid: NodeId, key: u64, value: Value, hash: ValueHash },
        }
        let mut persist_ops: Vec<PersistOp> = vec![];
        let mut changed             = 0usize;
        let mut coll_changed        = 0usize;
        let mut transforms_changed  = 0usize;
        let mut errors: Vec<(Uuid, TransformError)> = vec![];

        // ---- Pass 1: fan-out removal (sync, no .await) ----
        let mut fanout_seen: HashSet<NodeId> = HashSet::new();
        for o in &outputs {
            if let Some(fslot) = o.fanout_slot {
                if !fanout_seen.insert(o.node_id) { continue; }
                let meta = match topo.transform_meta(o.node_id) { Some(m) => m, None => continue };
                let mut ws = self.inner.write().unwrap();
                let current_keys: HashSet<u64> = meta.input_edges.get(fslot)
                    .map(|eids| eids.iter().flat_map(|&eid| ws.read_collection(eid).iter().map(|e| e.key)).collect())
                    .unwrap_or_default();
                let n_out = meta.erased.schema.outputs.len();
                for out_slot in 0..n_out {
                    let removed: HashSet<u64> = meta.output_edges.get(out_slot)
                        .map(|eids| eids.iter()
                            .flat_map(|&eid| ws.collection_keys(eid).into_iter().filter(|k| !current_keys.contains(k)))
                            .collect())
                        .unwrap_or_default();
                    if !removed.is_empty() {
                        if let Some(eids) = meta.output_edges.get(out_slot) {
                            for &eid in eids {
                                if ws.remove_elements(eid, &removed) {
                                    changed += 1;
                                    if let Some(edge) = topo.edge(eid) {
                                        Self::dirty_downstream_inner(&mut ws, topo, edge.to);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ---- Pass 2: apply outputs (sync, no .await) ----
        for o in outputs {
            if let Some(e) = o.error {
                self.inner.write().unwrap().mark_error(o.node_id, e.clone());
                errors.push((o.node_id.as_uuid(), e));
                continue;
            }
            let meta = match topo.transform_meta(o.node_id) { Some(m) => m, None => continue };
            let mut this_changed = false;

            for (out_slot, opt_out) in o.outputs.into_iter().enumerate() {
                let Some(out) = opt_out else { continue };
                let eids = match meta.output_edges.get(out_slot) {
                    Some(ids) => ids.clone(), None => continue,
                };
                match out {
                    OutputSlot::Single(v, h) => {
                        if let Some(fanout_key) = o.fanout_key {
                            for &eid in &eids {
                                let mut ws = self.inner.write().unwrap();
                                if ws.upsert_element(eid, fanout_key, v.clone(), h) {
                                    this_changed = true;
                                    if let Some(edge) = topo.edge(eid) {
                                        Self::dirty_downstream_inner(&mut ws, topo, edge.to);
                                        if let Endpoint::Io(nid) = edge.to {
                                            persist_ops.push(PersistOp::Element { nid, key: fanout_key, value: v.clone(), hash: h });
                                        }
                                    }
                                }
                            }
                        } else {
                            for &eid in &eids {
                                let mut ws = self.inner.write().unwrap();
                                if ws.write_single(eid, v.clone(), h) {
                                    this_changed = true;
                                    if let Some(edge) = topo.edge(eid) {
                                        Self::dirty_downstream_inner(&mut ws, topo, edge.to);
                                        if let Endpoint::Io(nid) = edge.to {
                                            if let Some(io_adj) = topo.io_adjacency(nid) {
                                                if let Some(in_eid) = io_adj.incoming {
                                                    ws.write_single(in_eid, v.clone(), h);
                                                }
                                            }
                                            persist_ops.push(PersistOp::Single { nid, value: v.clone(), hash: h });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    OutputSlot::Collection(pairs) => {
                        let n = pairs.len();
                        for &eid in &eids {
                            let mut ws = self.inner.write().unwrap();
                            if ws.replace_collection(eid, &pairs) {
                                this_changed = true;
                                coll_changed += n;
                                if let Some(edge) = topo.edge(eid) {
                                    Self::dirty_downstream_inner(&mut ws, topo, edge.to);
                                    if let Endpoint::Io(nid) = edge.to {
                                        for &(key, ref value, hash) in &pairs {
                                            persist_ops.push(PersistOp::Element { nid, key, value: value.clone(), hash });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(fslot) = o.fanout_slot {
                if let Some(eids) = meta.input_edges.get(fslot) {
                    let mut ws = self.inner.write().unwrap();
                    for &eid in eids { ws.clear_dirty_on_edge(eid); }
                }
            }
            if this_changed { changed += 1; transforms_changed += 1; }
            self.inner.write().unwrap().mark_clean(o.node_id);
        }

        // ---- Pass 3: async persistence (no locks held) ----
        for op in persist_ops {
            match op {
                PersistOp::Single  { nid, value, hash }        => { let _ = loader.persist(nid, &value, hash).await; }
                PersistOp::Element { nid, key, value, hash }   => { let _ = loader.persist_element(nid, key, &value, hash).await; }
            }
        }

        let nothing_happened = changed == 0 && errors.is_empty();
        PassResult { changed, coll_changed, transforms_evaluated: 0, transforms_changed, errors, nothing_happened }
    }

    // -----------------------------------------------------------------------
    // Snapshot / Restore (warm start)
    // -----------------------------------------------------------------------

    pub(crate) fn snapshot(&self, _topo: &Topology, registry: &ValueTypeRegistry) -> StateSnapshot {
        let ws = self.inner.read().unwrap();
        let node_status: HashMap<NodeId, bool> = ws.node_status.iter()
            .map(|(&id, s)| (id, s.is_dirty()))
            .collect();

        let mut edge_values: HashMap<EdgeId, SnapshotEdge> = HashMap::new();
        for (&eid, ev) in &ws.edge_values {
            let snap = match ev {
                EdgeValue::Single { value: Some(v), hash: Some(h), dirty: d } => {
                    let type_key    = registry.type_key_of(v);
                    let value_bytes = registry.serialize(v);
                    SnapshotEdge::Single { type_key, value_bytes, hash: *h, dirty: *d }
                }
                EdgeValue::Single { .. } => continue,
                EdgeValue::Collection(elems) => {
                    let snaps: Vec<SnapshotElement> = elems.iter().map(|el| {
                        SnapshotElement {
                            key:         el.key,
                            type_key:    registry.type_key_of(&el.value),
                            value_bytes: registry.serialize(&el.value),
                            hash:        el.hash,
                            dirty:       el.dirty,
                        }
                    }).collect();
                    SnapshotEdge::Collection(snaps)
                }
            };
            edge_values.insert(eid, snap);
        }
        StateSnapshot { node_status, edge_values }
    }

    pub(crate) fn restore(
        &self,
        topo:     &Topology,
        snapshot: StateSnapshot,
        registry: &ValueTypeRegistry,
    ) {
        let mut ws = self.inner.write().unwrap();

        // Re-init edges (in case topology changed).
        ws.edge_values.clear();
        for (eid, edge) in &topo.edges {
            ws.init_edge(*eid, edge.is_collection);
        }
        ws.node_status.clear();
        for id in topo.transform_nodes.keys() { ws.mark_dirty(*id); }

        // Restore snapshot values.
        for (eid, snap_edge) in snapshot.edge_values {
            match snap_edge {
                SnapshotEdge::Single { type_key, value_bytes, hash, dirty } => {
                    if let Ok(v) = registry.deserialize(&type_key, &value_bytes) {
                        ws.edge_values.insert(eid, EdgeValue::Single {
                            value: Some(v), hash: Some(hash), dirty,
                        });
                    }
                }
                SnapshotEdge::Collection(elems) => {
                    let items: Vec<CollectionElement> = elems.into_iter().filter_map(|e| {
                        registry.deserialize(&e.type_key, &e.value_bytes).ok().map(|v| {
                            CollectionElement { key: e.key, value: v, hash: e.hash, dirty: e.dirty }
                        })
                    }).collect();
                    ws.edge_values.insert(eid, EdgeValue::Collection(items));
                }
            }
        }

        // Restore node statuses.
        for (nid, dirty) in snapshot.node_status {
            if dirty { ws.mark_dirty(nid); } else { ws.mark_clean(nid); }
        }
    }

    pub(crate) fn clear_and_mark_all_dirty(&self, topo: &Topology) {
        let mut ws = self.inner.write().unwrap();
        ws.clear_all_values();
        for id in topo.transform_nodes.keys() { ws.mark_dirty(*id); }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn dirty_downstream_inner(ws: &mut WorkStateInner, topo: &Topology, endpoint: Endpoint) {
        match endpoint {
            Endpoint::TransformInput { transform, .. } => { ws.mark_dirty(transform); }
            Endpoint::Io(io_n) => {
                if let Some(adj) = topo.io_adjacency(io_n) {
                    for &eid in &adj.outgoing {
                        if let Some(edge) = topo.edge(eid) {
                            if let Endpoint::TransformInput { transform, .. } = edge.to {
                                ws.mark_dirty(transform);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn read_single_for_slot_inner(
        ws: &WorkStateInner, topo: &Topology, tid: NodeId, slot: usize,
    ) -> Option<(Value, ValueHash)> {
        let meta = topo.transform_meta(tid)?;
        let eids = meta.input_edges.get(slot)?;
        for &eid in eids {
            if let Some(v) = ws.read_single(eid) { return Some(v); }
            // IoNode passthrough.
            if let Some(edge) = topo.edge(eid) {
                if let Endpoint::Io(io_n) = edge.from {
                    if let Some(io_adj) = topo.io_adjacency(io_n) {
                        if let Some(in_eid) = io_adj.incoming {
                            if let Some(v) = ws.read_single(in_eid) { return Some(v); }
                        }
                    }
                }
            }
        }
        None
    }

    fn read_other_single_inputs(
        &self,
        topo:      &Topology,
        tid:       NodeId,
        skip_slot: usize,
        dispatch:  &crate::transform::DispatchTable,
        registry:  &Arc<ValueTypeRegistry>,
    ) -> Option<Vec<(usize, Box<dyn std::any::Any + Send + Sync>)>> {
        let ws = self.inner.read().unwrap();
        let meta = topo.transform_meta(tid)?;
        let schema = meta.layout();
        let mut result = vec![];
        for (slot, sinfo) in schema.inputs.iter().enumerate() {
            if slot == skip_slot || sinfo.is_col { continue; }
            let (v, _) = Self::read_single_for_slot_inner(&ws, topo, tid, slot)?;
            let dc = dispatch.inputs[slot].downcast;
            let boxed = dc(&v, registry).ok()?;
            result.push((slot, boxed));
        }
        Some(result)
    }

    fn read_all_inputs(
        &self,
        topo:     &Topology,
        tid:      NodeId,
        dispatch: &crate::transform::DispatchTable,
        registry: &Arc<ValueTypeRegistry>,
    ) -> Option<(
        Vec<Option<Box<dyn std::any::Any + Send + Sync>>>,
        Vec<Option<crate::transform::ErasedCollection>>,
    )> {
        let ws = self.inner.read().unwrap();
        let meta = topo.transform_meta(tid)?;
        let schema = meta.layout();
        let n_in = schema.inputs.len();
        let mut single_inputs:     Vec<Option<Box<dyn std::any::Any + Send + Sync>>> =
            (0..n_in).map(|_| None).collect();
        let mut collection_inputs: Vec<Option<crate::transform::ErasedCollection>> =
            (0..n_in).map(|_| None).collect();

        for (slot, sinfo) in schema.inputs.iter().enumerate() {
            if sinfo.is_col {
                let eids = meta.input_edges.get(slot)?;
                let mut all: Vec<crate::workstate::CollectionElement> = vec![];
                for &eid in eids { all.extend(ws.read_collection(eid).iter().cloned()); }
                if let Some(build) = dispatch.inputs[slot].build_collection {
                    let dirty_keys: Vec<u64> = all.iter().filter(|e| e.dirty).map(|e| e.key).collect();
                    let all_values: Vec<Value> = all.iter().map(|e| e.value.clone()).collect();
                    collection_inputs[slot] = Some(build(all_values, dirty_keys, registry));
                }
            } else {
                let (v, _) = Self::read_single_for_slot_inner(&ws, topo, tid, slot)?;
                let dc = dispatch.inputs[slot].downcast;
                single_inputs[slot] = Some(dc(&v, registry).ok()?);
            }
        }
        Some((single_inputs, collection_inputs))
    }
}

// ---------------------------------------------------------------------------
// TransformPayload — self-contained transform execution unit
// ---------------------------------------------------------------------------

struct TransformPayload {
    node_id:          NodeId,
    erased:           ErasedTransform,
    single_inputs:    Vec<Option<Box<dyn std::any::Any + Send + Sync>>>,
    collection_inputs:Vec<Option<crate::transform::ErasedCollection>>,
    registry:         Arc<ValueTypeRegistry>,
    fanout_key:       Option<u64>,
    fanout_slot:      Option<usize>,
    sink:             Arc<SegQueue<TaskOutput>>,
}

unsafe impl Send for TransformPayload {}
unsafe impl Sync for TransformPayload {}

impl TransformPayload {
    async fn run(self) {
        let schema   = Arc::clone(&self.erased.schema);
        let dispatch = Arc::clone(&self.erased.dispatch);
        let mut ctx  = TransformContext::new(
            Arc::clone(&schema),
            Arc::clone(&dispatch),
            Arc::clone(&self.registry),
        );
        ctx.single_inputs     = self.single_inputs;
        ctx.collection_inputs = self.collection_inputs;

        let result = self.erased.apply(&mut ctx).await;

        let outputs: Vec<Option<OutputSlot>> = match &result {
            Ok(_) => ctx.outputs.into_iter().map(|opt| {
                opt.map(|o| match o {
                    ContextOutput::Single(v, h)      => OutputSlot::Single(v, h),
                    ContextOutput::Collection(pairs) => OutputSlot::Collection(pairs),
                })
            }).collect(),
            Err(_) => vec![],
        };

        self.sink.push(TaskOutput {
            node_id:     self.node_id,
            outputs,
            error:       result.err(),
            fanout_key:  self.fanout_key,
            fanout_slot: self.fanout_slot,
        });
    }
}
