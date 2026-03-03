//! Bipartite computation graph: IoNodes, TransformNodes, and ValueEdges.
//! All types are `pub(crate)`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::node_id::NodeId;
use crate::transform::{ErasedTransform, TransformError};
use crate::value::{Value, ValueHash};

// ---------------------------------------------------------------------------
// EdgeId
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EdgeId(u64);

static EDGE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl EdgeId {
    pub(crate) fn next() -> Self {
        Self(EDGE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

// ---------------------------------------------------------------------------
// Endpoint
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Endpoint {
    Io(NodeId),
    TransformInput  { transform: NodeId, slot: usize },
    TransformOutput { transform: NodeId, slot: usize },
}

// ---------------------------------------------------------------------------
// NodeStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum NodeStatus {
    Dirty,
    Clean,
    Error(TransformError),
}

impl NodeStatus {
    pub(crate) fn is_dirty(&self) -> bool { matches!(self, NodeStatus::Dirty) }
}

// ---------------------------------------------------------------------------
// SingleEdgeValue / CollectionElement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct SingleEdgeValue {
    pub value: Option<Value>,
    pub hash:  Option<ValueHash>,
    pub dirty: bool,
}

impl Default for SingleEdgeValue {
    fn default() -> Self { Self { value: None, hash: None, dirty: false } }
}

/// One element in a collection edge.
#[derive(Debug, Clone)]
pub(crate) struct CollectionElement {
    pub key:   u64,
    pub value: Value,
    pub hash:  ValueHash,
    pub dirty: bool,
}

// ---------------------------------------------------------------------------
// EdgePayload
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum EdgePayload {
    Single(SingleEdgeValue),
    Collection(Vec<CollectionElement>),
}

impl EdgePayload {
    pub(crate) fn single() -> Self { Self::Single(Default::default()) }
    pub(crate) fn collection() -> Self { Self::Collection(vec![]) }
    pub(crate) fn is_collection(&self) -> bool { matches!(self, EdgePayload::Collection(_)) }
}

// ---------------------------------------------------------------------------
// ValueEdge
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct ValueEdge {
    pub id:      EdgeId,
    pub from:    Endpoint,
    pub to:      Endpoint,
    pub payload: EdgePayload,
}

// ---------------------------------------------------------------------------
// IoNode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoKind { Input, Output }

#[derive(Debug, Clone)]
pub(crate) struct IoNode {
    pub id:       NodeId,
    pub kind:     IoKind,
    /// EdgeId of the one incoming edge (None for Input nodes initially).
    pub incoming: Option<EdgeId>,
    /// EdgeIds of all outgoing edges.
    pub outgoing: Vec<EdgeId>,
}

// ---------------------------------------------------------------------------
// TransformNode
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct TransformNode {
    pub id:            NodeId,
    pub transform_key: String,
    pub transform:     ErasedTransform,
    /// input_edges[slot] = list of incoming EdgeIds for that slot.
    pub input_edges:   Vec<Vec<EdgeId>>,
    /// output_edges[slot] = list of outgoing EdgeIds for that slot.
    pub output_edges:  Vec<Vec<EdgeId>>,
    pub status:        NodeStatus,
}

// ---------------------------------------------------------------------------
// GraphError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum GraphError {
    DuplicateNode(NodeId),
    NodeNotFound(NodeId),
    EdgeNotFound(EdgeId),
    SlotOutOfRange { node: NodeId, slot: usize },
    DuplicateEdge { from: Endpoint, to: Endpoint },
    TypeConflict(String),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::DuplicateNode(id)          => write!(f, "duplicate node {id}"),
            GraphError::NodeNotFound(id)           => write!(f, "node {id} not found"),
            GraphError::EdgeNotFound(id)           => write!(f, "edge {id:?} not found"),
            GraphError::SlotOutOfRange { node, slot } =>
                write!(f, "slot {slot} out of range on node {node}"),
            GraphError::DuplicateEdge { from, to } =>
                write!(f, "duplicate edge {from:?} → {to:?}"),
            GraphError::TypeConflict(msg)          => write!(f, "type conflict: {msg}"),
        }
    }
}
impl std::error::Error for GraphError {}

// ---------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------

/// The bipartite computation graph.  All mutations are guarded by a single
/// `RwLock` so that topology is always consistent.  After `EngineBuilder::build()`
/// topology is sealed and only value mutations occur (via `set_input` / `push_output`).
#[derive(Debug)]
pub(crate) struct Graph {
    inner: RwLock<GraphInner>,
}

#[derive(Debug, Default)]
struct GraphInner {
    io_nodes:        HashMap<NodeId, IoNode>,
    transform_nodes: HashMap<NodeId, TransformNode>,
    edges:           HashMap<EdgeId, ValueEdge>,
}

impl Graph {
    pub(crate) fn new() -> Self {
        Self { inner: RwLock::new(GraphInner::default()) }
    }

    // -----------------------------------------------------------------------
    // Node addition (build phase only)
    // -----------------------------------------------------------------------

    pub(crate) fn add_input_node(&self, id: NodeId) -> Result<(), GraphError> {
        let mut g = self.inner.write().unwrap();
        if g.io_nodes.contains_key(&id) || g.transform_nodes.contains_key(&id) {
            return Err(GraphError::DuplicateNode(id));
        }
        g.io_nodes.insert(id, IoNode { id, kind: IoKind::Input, incoming: None, outgoing: vec![] });
        Ok(())
    }

    pub(crate) fn add_output_node(&self, id: NodeId) -> Result<(), GraphError> {
        let mut g = self.inner.write().unwrap();
        if g.io_nodes.contains_key(&id) || g.transform_nodes.contains_key(&id) {
            return Err(GraphError::DuplicateNode(id));
        }
        g.io_nodes.insert(id, IoNode { id, kind: IoKind::Output, incoming: None, outgoing: vec![] });
        Ok(())
    }

    pub(crate) fn add_transform_node(
        &self, id: NodeId, key: String, transform: ErasedTransform,
    ) -> Result<(), GraphError> {
        let n_in  = transform.schema.inputs.len();
        let n_out = transform.schema.outputs.len();
        let mut g = self.inner.write().unwrap();
        if g.io_nodes.contains_key(&id) || g.transform_nodes.contains_key(&id) {
            return Err(GraphError::DuplicateNode(id));
        }
        g.transform_nodes.insert(id, TransformNode {
            id,
            transform_key: key,
            transform,
            input_edges:   vec![vec![]; n_in],
            output_edges:  vec![vec![]; n_out],
            status:        NodeStatus::Dirty,
        });
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Edge addition (build phase only)
    // -----------------------------------------------------------------------

    /// Add an edge from `from` to `to`.  `collection` controls edge type.
    /// The edge type may also be forced Collection if the target slot schema
    /// demands it.
    pub(crate) fn add_edge(
        &self,
        from: Endpoint,
        to:   Endpoint,
        collection: bool,
    ) -> Result<EdgeId, GraphError> {
        let id = EdgeId::next();
        let payload = if collection { EdgePayload::collection() } else { EdgePayload::single() };
        let edge = ValueEdge { id, from, to, payload };

        let mut g = self.inner.write().unwrap();

        // Check for duplicate
        for e in g.edges.values() {
            if e.from == from && e.to == to {
                return Err(GraphError::DuplicateEdge { from, to });
            }
        }

        // Register on source node outgoing list.
        match from {
            Endpoint::Io(n) => {
                g.io_nodes.get_mut(&n)
                    .ok_or(GraphError::NodeNotFound(n))?
                    .outgoing.push(id);
            }
            Endpoint::TransformOutput { transform, slot } => {
                let t = g.transform_nodes.get_mut(&transform)
                    .ok_or(GraphError::NodeNotFound(transform))?;
                if slot >= t.output_edges.len() {
                    return Err(GraphError::SlotOutOfRange { node: transform, slot });
                }
                t.output_edges[slot].push(id);
            }
            Endpoint::TransformInput { .. } => {
                return Err(GraphError::TypeConflict(
                    "TransformInput cannot be an edge source".into()
                ));
            }
        }

        // Register on target node incoming list.
        match to {
            Endpoint::Io(n) => {
                let node = g.io_nodes.get_mut(&n)
                    .ok_or(GraphError::NodeNotFound(n))?;
                node.incoming = Some(id);
            }
            Endpoint::TransformInput { transform, slot } => {
                let t = g.transform_nodes.get_mut(&transform)
                    .ok_or(GraphError::NodeNotFound(transform))?;
                if slot >= t.input_edges.len() {
                    return Err(GraphError::SlotOutOfRange { node: transform, slot });
                }
                t.input_edges[slot].push(id);
            }
            Endpoint::TransformOutput { .. } => {
                return Err(GraphError::TypeConflict(
                    "TransformOutput cannot be an edge target".into()
                ));
            }
        }

        g.edges.insert(id, edge);
        Ok(id)
    }

    // -----------------------------------------------------------------------
    // Input value mutation (runtime)
    // -----------------------------------------------------------------------

    /// Pre-load a restored input value on warm start.  Sets value on outgoing
    /// edges WITHOUT dirtying downstream transforms.  This lets `set_input`
    /// later compare hashes correctly and skip recomputation when unchanged.
    pub(crate) fn preload_input(&self, id: NodeId, value: Value, hash: ValueHash) {
        let mut g = self.inner.write().unwrap();
        let outgoing: Vec<EdgeId> = g.io_nodes.get(&id)
            .map(|n| n.outgoing.clone())
            .unwrap_or_default();
        for eid in &outgoing {
            if let Some(edge) = g.edges.get_mut(eid) {
                if let EdgePayload::Single(sv) = &mut edge.payload {
                    sv.value = Some(value.clone());
                    sv.hash  = Some(hash);
                    sv.dirty = false; // NOT dirty — warm start pre-load
                }
            }
        }
    }

    /// Set the value of an input IoNode's outgoing edge(s).
    /// Returns `true` if the hash changed (node was actually dirtied).
    pub(crate) fn set_input(
        &self,
        id: NodeId,
        value: Value,
        hash:  ValueHash,
    ) -> bool {
        let mut g = self.inner.write().unwrap();
        // Dirty all downstream transform nodes if hash changed.
        let outgoing: Vec<EdgeId> = g.io_nodes.get(&id)
            .map(|n| n.outgoing.clone())
            .unwrap_or_default();

        let mut changed = false;
        for eid in &outgoing {
            if let Some(edge) = g.edges.get_mut(eid) {
                match &mut edge.payload {
                    EdgePayload::Single(sv) => {
                        if sv.hash.map(|h| h != hash).unwrap_or(true) {
                            sv.value = Some(value.clone());
                            sv.hash  = Some(hash);
                            sv.dirty = true;
                            changed  = true;
                        }
                    }
                    EdgePayload::Collection(_) => {
                        // Input IoNode → Collection edge not expected in this flow.
                        // The expand transform handles collection fan-out.
                    }
                }
            }
        }

        // Also dirty the transform nodes that have these edges as inputs.
        if changed {
            for eid in &outgoing {
                if let Some(edge) = g.edges.get(eid) {
                    if let Endpoint::TransformInput { transform, .. } = edge.to {
                        if let Some(t) = g.transform_nodes.get_mut(&transform) {
                            t.status = NodeStatus::Dirty;
                        }
                    }
                }
            }
        }
        changed
    }

    // -----------------------------------------------------------------------
    // Output value storage (after scheduler computes a transform)
    // -----------------------------------------------------------------------

    /// Store a single output value on the edges coming from `transform` slot `out_slot`.
    /// Dirty downstream transforms if hash changed.
    pub(crate) fn push_single_output(
        &self,
        transform: NodeId,
        out_slot:  usize,
        value:     Value,
        hash:      ValueHash,
    ) -> bool {
        let mut g = self.inner.write().unwrap();
        let edge_ids: Vec<EdgeId> = g.transform_nodes.get(&transform)
            .and_then(|t| t.output_edges.get(out_slot))
            .cloned()
            .unwrap_or_default();

        let mut changed = false;
        for eid in &edge_ids {
            if let Some(edge) = g.edges.get_mut(eid) {
                match &mut edge.payload {
                    EdgePayload::Single(sv) => {
                        if sv.hash.map(|h| h != hash).unwrap_or(true) {
                            sv.value = Some(value.clone());
                            sv.hash  = Some(hash);
                            sv.dirty = true;
                            changed  = true;
                        }
                    }
                    EdgePayload::Collection(elems) => {
                        // Single→Collection: treat the value as one element.
                        // The element key is derived externally; use hash as key proxy.
                        // This path is used by per-element fan-out.
                        let key = hash; // element key = output hash (stable per element)
                        if let Some(el) = elems.iter_mut().find(|e| e.key == key) {
                            if el.hash != hash {
                                el.value = value.clone();
                                el.hash  = hash;
                                el.dirty = true;
                                changed  = true;
                            }
                        } else {
                            elems.push(CollectionElement { key, value: value.clone(), hash, dirty: true });
                            changed = true;
                        }
                    }
                }
            }
        }

        if changed {
            // Collect downstream transform IDs first (avoids split borrow).
            let dirty_targets: Vec<NodeId> = edge_ids.iter()
                .filter_map(|eid| g.edges.get(eid))
                .filter_map(|e| if let Endpoint::TransformInput { transform, .. } = e.to {
                    Some(transform)
                } else { None })
                .collect();
            for t_id in dirty_targets {
                if let Some(t) = g.transform_nodes.get_mut(&t_id) {
                    t.status = NodeStatus::Dirty;
                }
            }
        }
        changed
    }

    /// Store collection output from `transform` slot `out_slot`.
    /// Diffs against existing collection; dirties downstream transforms.
    /// Used by gather/expand transforms that replace the entire collection.
    pub(crate) fn push_collection_output(
        &self,
        transform: NodeId,
        out_slot:  usize,
        items:     Vec<(u64, Value, ValueHash)>,
    ) -> bool {
        let mut g = self.inner.write().unwrap();
        let edge_ids: Vec<EdgeId> = g.transform_nodes.get(&transform)
            .and_then(|t| t.output_edges.get(out_slot))
            .cloned()
            .unwrap_or_default();

        let mut changed = false;
        for eid in &edge_ids {
            if let Some(edge) = g.edges.get_mut(eid) {
                if let EdgePayload::Collection(elems) = &mut edge.payload {
                    let new_keys: std::collections::HashSet<u64> =
                        items.iter().map(|(k, _, _)| *k).collect();
                    // Mark removed elements dirty (absence = removal for downstream).
                    elems.retain(|el| new_keys.contains(&el.key) || {
                        changed = true;
                        false // actually remove them
                    });
                    for (key, value, hash) in &items {
                        if let Some(el) = elems.iter_mut().find(|e| e.key == *key) {
                            if el.hash != *hash {
                                el.value = value.clone();
                                el.hash  = *hash;
                                el.dirty = true;
                                changed  = true;
                            }
                        } else {
                            elems.push(CollectionElement {
                                key: *key, value: value.clone(), hash: *hash, dirty: true,
                            });
                            changed = true;
                        }
                    }
                }
            }
        }

        if changed {
            let dirty_targets: Vec<NodeId> = edge_ids.iter()
                .filter_map(|eid| g.edges.get(eid))
                .filter_map(|e| if let Endpoint::TransformInput { transform, .. } = e.to {
                    Some(transform)
                } else { None })
                .collect();
            for t_id in dirty_targets {
                if let Some(t) = g.transform_nodes.get_mut(&t_id) {
                    t.status = NodeStatus::Dirty;
                }
            }
        }
        changed
    }

    /// Upsert collection elements for a fan-out transform output.
    /// Unlike `push_collection_output`, this does NOT remove elements that are
    /// absent from `items` — it only adds new elements or updates changed ones.
    /// Removal is handled by `remove_collection_elements`.
    pub(crate) fn upsert_collection_output(
        &self,
        transform: NodeId,
        out_slot:  usize,
        items:     Vec<(u64, Value, ValueHash)>,
    ) -> bool {
        let mut g = self.inner.write().unwrap();
        let edge_ids: Vec<EdgeId> = g.transform_nodes.get(&transform)
            .and_then(|t| t.output_edges.get(out_slot))
            .cloned()
            .unwrap_or_default();

        let mut changed = false;
        for eid in &edge_ids {
            if let Some(edge) = g.edges.get_mut(eid) {
                if let EdgePayload::Collection(elems) = &mut edge.payload {
                    for (key, value, hash) in &items {
                        if let Some(el) = elems.iter_mut().find(|e| e.key == *key) {
                            if el.hash != *hash {
                                el.value = value.clone();
                                el.hash  = *hash;
                                el.dirty = true;
                                changed  = true;
                            }
                        } else {
                            elems.push(CollectionElement {
                                key: *key, value: value.clone(), hash: *hash, dirty: true,
                            });
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            let dirty_targets: Vec<NodeId> = edge_ids.iter()
                .filter_map(|eid| g.edges.get(eid))
                .filter_map(|e| if let Endpoint::TransformInput { transform, .. } = e.to {
                    Some(transform)
                } else { None })
                .collect();
            for t_id in dirty_targets {
                if let Some(t) = g.transform_nodes.get_mut(&t_id) {
                    t.status = NodeStatus::Dirty;
                }
            }
        }
        changed
    }

    /// Remove specific collection elements from a fan-out transform output.
    /// Called when an upstream collection element has been removed.
    pub(crate) fn remove_collection_elements(
        &self,
        transform: NodeId,
        out_slot:  usize,
        keys_to_remove: &[u64],
    ) -> bool {
        if keys_to_remove.is_empty() { return false; }
        let remove_set: std::collections::HashSet<u64> = keys_to_remove.iter().copied().collect();
        let mut g = self.inner.write().unwrap();
        let edge_ids: Vec<EdgeId> = g.transform_nodes.get(&transform)
            .and_then(|t| t.output_edges.get(out_slot))
            .cloned()
            .unwrap_or_default();

        let mut changed = false;
        for eid in &edge_ids {
            if let Some(edge) = g.edges.get_mut(eid) {
                if let EdgePayload::Collection(elems) = &mut edge.payload {
                    let before = elems.len();
                    elems.retain(|el| !remove_set.contains(&el.key));
                    if elems.len() < before { changed = true; }
                }
            }
        }
        if changed {
            let dirty_targets: Vec<NodeId> = edge_ids.iter()
                .filter_map(|eid| g.edges.get(eid))
                .filter_map(|e| if let Endpoint::TransformInput { transform, .. } = e.to {
                    Some(transform)
                } else { None })
                .collect();
            for t_id in dirty_targets {
                if let Some(t) = g.transform_nodes.get_mut(&t_id) {
                    t.status = NodeStatus::Dirty;
                }
            }
        }
        changed
    }

    fn dirty_downstream_of_edges_locked(
        &self,
        edges: &HashMap<EdgeId, ValueEdge>,
        edge_ids: &[EdgeId],
        transforms: &mut HashMap<NodeId, TransformNode>,
    ) {
        let _ = (edges, edge_ids, transforms); // kept for potential future use
    }

    // -----------------------------------------------------------------------
    // Status
    // -----------------------------------------------------------------------

    pub(crate) fn mark_clean(&self, id: NodeId) {
        if let Some(t) = self.inner.write().unwrap().transform_nodes.get_mut(&id) {
            t.status = NodeStatus::Clean;
        }
    }

    pub(crate) fn mark_error(&self, id: NodeId, err: TransformError) {
        if let Some(t) = self.inner.write().unwrap().transform_nodes.get_mut(&id) {
            t.status = NodeStatus::Error(err);
        }
    }

    /// Mark all transform nodes Clean.  Called on warm start after restoring
    /// cached values.  Transforms will be re-dirtied by `set_input()` if
    /// any upstream input hash has changed.
    pub(crate) fn mark_all_clean_if_warmed(&self) {
        let mut g = self.inner.write().unwrap();
        // Only mark clean if at least one output node has a value (i.e. it's a real warm start).
        let has_output = g.io_nodes.values().any(|n| {
            n.incoming.and_then(|eid| g.edges.get(&eid))
                .and_then(|e| if let EdgePayload::Single(sv) = &e.payload { sv.value.as_ref() } else { None })
                .is_some()
        });
        if has_output {
            for t in g.transform_nodes.values_mut() {
                t.status = NodeStatus::Clean;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Read helpers
    // -----------------------------------------------------------------------

    /// Return all dirty transform node IDs in topological order.
    pub(crate) fn dirty_transforms_topo(&self) -> Vec<NodeId> {
        let g = self.inner.read().unwrap();
        // Build adjacency: transform → transforms it feeds.
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
        for id in g.transform_nodes.keys() { in_degree.entry(*id).or_insert(0); }

        // Build edge index: IoNode → downstream endpoints, so we can traverse
        // TransformOutput → IoNode → TransformInput indirect paths.
        let mut io_out_endpoints: HashMap<NodeId, Vec<Endpoint>> = HashMap::new();
        for edge in g.edges.values() {
            if let Endpoint::Io(n) = edge.from {
                io_out_endpoints.entry(n).or_default().push(edge.to);
            }
        }

        let mut seen_pairs: std::collections::HashSet<(NodeId, NodeId)> =
            std::collections::HashSet::new();

        for edge in g.edges.values() {
            let src_t = match edge.from {
                Endpoint::TransformOutput { transform, .. } => Some(transform),
                _ => None,
            };
            if let Some(s) = src_t {
                // Direct TransformOutput → TransformInput.
                if let Endpoint::TransformInput { transform: d, .. } = edge.to {
                    if seen_pairs.insert((s, d)) {
                        adj.entry(s).or_default().push(d);
                        *in_degree.entry(d).or_insert(0) += 1;
                    }
                }
                // Indirect: TransformOutput → IoNode → TransformInput.
                if let Endpoint::Io(io_n) = edge.to {
                    if let Some(downstreams) = io_out_endpoints.get(&io_n) {
                        for &downstream in downstreams {
                            if let Endpoint::TransformInput { transform: d, .. } = downstream {
                                if seen_pairs.insert((s, d)) {
                                    adj.entry(s).or_default().push(d);
                                    *in_degree.entry(d).or_insert(0) += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Kahn's algorithm.
        let mut queue: std::collections::VecDeque<NodeId> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut order = vec![];
        while let Some(id) = queue.pop_front() {
            order.push(id);
            if let Some(nexts) = adj.get(&id) {
                for &next in nexts {
                    let deg = in_degree.get_mut(&next).unwrap();
                    *deg -= 1;
                    if *deg == 0 { queue.push_back(next); }
                }
            }
        }

        // Filter to dirty only (preserve topo order).
        order.into_iter()
            .filter(|id| g.transform_nodes.get(id).map(|t| t.status.is_dirty()).unwrap_or(false))
            .collect()
    }

    /// Read the incoming single-value edge for a given transform input slot.
    /// Returns `None` if no value available yet.
    pub(crate) fn read_single_input(
        &self, transform: NodeId, slot: usize,
    ) -> Option<(Value, ValueHash)> {
        let g = self.inner.read().unwrap();
        let edge_ids = g.transform_nodes.get(&transform)?.input_edges.get(slot)?.clone();
        for eid in &edge_ids {
            if let Some(edge) = g.edges.get(eid) {
                if let EdgePayload::Single(sv) = &edge.payload {
                    if let (Some(v), Some(h)) = (&sv.value, &sv.hash) {
                        return Some((v.clone(), *h));
                    }
                }
            }
        }
        None
    }

    /// Read all collection elements for a given transform input slot.
    pub(crate) fn read_collection_input(
        &self, transform: NodeId, slot: usize,
    ) -> Vec<CollectionElement> {
        let g = self.inner.read().unwrap();
        let edge_ids = match g.transform_nodes.get(&transform)
            .and_then(|t| t.input_edges.get(slot)) {
            Some(ids) => ids.clone(),
            None => return vec![],
        };
        let mut all = vec![];
        for eid in &edge_ids {
            if let Some(edge) = g.edges.get(eid) {
                match &edge.payload {
                    EdgePayload::Single(sv) => {
                        // Single→Collection crossing: treat as one element.
                        if let (Some(v), Some(h)) = (&sv.value, &sv.hash) {
                            all.push(CollectionElement {
                                key: *h, // use hash as key
                                value: v.clone(), hash: *h, dirty: sv.dirty,
                            });
                        }
                    }
                    EdgePayload::Collection(elems) => {
                        all.extend_from_slice(elems);
                    }
                }
            }
        }
        all
    }

    /// Get the transform node for an id (cloned schema for inspection).
    pub(crate) fn transform_schema(&self, id: NodeId) -> Option<Arc<crate::transform::TransformSchema>> {
        self.inner.read().unwrap().transform_nodes.get(&id)
            .map(|t| Arc::clone(&t.transform.schema))
    }

    /// Get all transform node IDs.
    pub(crate) fn all_transform_ids(&self) -> Vec<NodeId> {
        self.inner.read().unwrap().transform_nodes.keys().cloned().collect()
    }

    /// Get all IoNode IDs (input and output).
    pub(crate) fn all_io_ids(&self) -> Vec<(NodeId, IoKind)> {
        self.inner.read().unwrap().io_nodes.values()
            .map(|n| (n.id, n.kind)).collect()
    }

    /// Clear all in-memory edge values (Single payloads).
    /// Called by `Engine::discard()` so that subsequent `get()` calls read
    /// from storage rather than stale in-memory graph state.
    pub(crate) fn clear_io_values(&self) {
        let mut g = self.inner.write().unwrap();
        for edge in g.edges.values_mut() {
            if let EdgePayload::Single(sv) = &mut edge.payload {
                sv.value = None;
                sv.hash  = None;
                sv.dirty = false;
            }
        }
        // Mark all transforms dirty so they re-evaluate next update.
        for t in g.transform_nodes.values_mut() {
            t.status = NodeStatus::Dirty;
        }
    }

    /// Read the outgoing Single edge value of an IoNode (output node).
    pub(crate) fn peek_output(&self, id: NodeId) -> Option<(Value, ValueHash)> {
        let g = self.inner.read().unwrap();
        let node = g.io_nodes.get(&id)?;
        // Output node has one incoming edge.
        let eid = node.incoming?;
        let edge = g.edges.get(&eid)?;
        match &edge.payload {
            EdgePayload::Single(sv) => sv.value.as_ref().map(|v| (v.clone(), sv.hash.unwrap_or(0))),
            _ => None,
        }
    }

    /// Store output value on the IoNode (via its incoming edge).
    pub(crate) fn store_output_on_node(
        &self, node_id: NodeId, value: Value, hash: ValueHash,
    ) {
        let mut g = self.inner.write().unwrap();

        // Write the value to the incoming edge of the IoNode (so peek_output works).
        if let Some(node) = g.io_nodes.get(&node_id) {
            if let Some(eid) = node.incoming {
                if let Some(edge) = g.edges.get_mut(&eid) {
                    if let EdgePayload::Single(sv) = &mut edge.payload {
                        sv.value = Some(value.clone());
                        sv.hash  = Some(hash);
                        sv.dirty = false;
                    }
                }
            }
        }

        // Also propagate to all outgoing edges FROM this IoNode,
        // and dirty any downstream transforms (chained pipelines).
        let outgoing: Vec<EdgeId> = g.io_nodes.get(&node_id)
            .map(|n| n.outgoing.clone())
            .unwrap_or_default();

        let mut dirty_ids: Vec<NodeId> = vec![];
        for eid in &outgoing {
            if let Some(edge) = g.edges.get_mut(eid) {
                match &mut edge.payload {
                    EdgePayload::Single(sv) => {
                        if sv.hash.map(|h| h != hash).unwrap_or(true) {
                            sv.value = Some(value.clone());
                            sv.hash  = Some(hash);
                            sv.dirty = true;
                            if let Endpoint::TransformInput { transform, .. } = edge.to {
                                dirty_ids.push(transform);
                            }
                        }
                    }
                    EdgePayload::Collection(_) => {
                        // Collection output from an IoNode is handled by push_collection_output.
                    }
                }
            }
        }
        for t_id in dirty_ids {
            if let Some(t) = g.transform_nodes.get_mut(&t_id) {
                t.status = NodeStatus::Dirty;
            }
        }
    }

    /// Mark an input IoNode's outgoing edges as clean after they've been read.
    pub(crate) fn clear_input_dirty(&self, transform: NodeId, slot: usize) {
        let mut g = self.inner.write().unwrap();
        let edge_ids: Vec<EdgeId> = g.transform_nodes.get(&transform)
            .and_then(|t| t.input_edges.get(slot))
            .cloned()
            .unwrap_or_default();
        for eid in edge_ids {
            if let Some(edge) = g.edges.get_mut(&eid) {
                match &mut edge.payload {
                    EdgePayload::Single(sv) => sv.dirty = false,
                    EdgePayload::Collection(elems) => {
                        for el in elems.iter_mut() { el.dirty = false; }
                    }
                }
            }
        }
    }

    /// Return a snapshot of the current graph topology for persistence.
    pub(crate) fn snapshot_topology(&self)
        -> (Vec<(NodeId, IoKind)>, Vec<(NodeId, String)>, Vec<(Endpoint, Endpoint, bool)>)
    {
        let g = self.inner.read().unwrap();
        let io = g.io_nodes.values().map(|n| (n.id, n.kind)).collect();
        let tx = g.transform_nodes.values().map(|t| (t.id, t.transform_key.clone())).collect();
        let ed = g.edges.values().map(|e| (e.from, e.to, e.payload.is_collection())).collect();
        (io, tx, ed)
    }

    /// Invoke the erased transform apply function (gives the scheduler access).
    pub(crate) fn get_erased_transform(&self, id: NodeId) -> Option<ErasedTransform> {
        self.inner.read().unwrap().transform_nodes.get(&id)
            .map(|t| t.transform.clone())
    }

    /// Check whether a transform node exists.
    pub(crate) fn has_transform(&self, id: NodeId) -> bool {
        self.inner.read().unwrap().transform_nodes.contains_key(&id)
    }

    /// Get all output edge endpoint pairs for a transform output slot.
    pub(crate) fn output_edge_targets(&self, transform: NodeId, slot: usize) -> Vec<(EdgeId, Endpoint)> {
        let g = self.inner.read().unwrap();
        g.transform_nodes.get(&transform)
            .and_then(|t| t.output_edges.get(slot))
            .map(|eids| eids.iter().filter_map(|&eid| {
                g.edges.get(&eid).map(|e| (eid, e.to))
            }).collect())
            .unwrap_or_default()
    }

    /// Get the input edge slot kind (is it collection?) for a transform.
    pub(crate) fn input_slot_is_collection(&self, transform: NodeId, slot: usize) -> bool {
        let g = self.inner.read().unwrap();
        if let Some(t) = g.transform_nodes.get(&transform) {
            if let Some(eids) = t.input_edges.get(slot) {
                if let Some(&eid) = eids.first() {
                    if let Some(edge) = g.edges.get(&eid) {
                        return edge.payload.is_collection();
                    }
                }
            }
            // Fall back to schema.
            t.transform.schema.inputs.get(slot).map(|s| s.is_col).unwrap_or(false)
        } else {
            false
        }
    }

    /// Return all edges (from, to, is_collection) for topology persistence.
    pub(crate) fn all_edges(&self) -> Vec<(Endpoint, Endpoint, bool)> {
        self.inner.read().unwrap().edges.values()
            .map(|e| (e.from, e.to, e.payload.is_collection()))
            .collect()
    }

    /// Return the source transform NodeIds that feed a given transform's input slot.
    /// Traverses through intermediate IoNodes (e.g. T1 → IoNode → T2 slot).
    pub(crate) fn input_slot_sources(&self, transform: NodeId, slot: usize) -> Vec<NodeId> {
        let g = self.inner.read().unwrap();
        let edge_ids = match g.transform_nodes.get(&transform)
            .and_then(|t| t.input_edges.get(slot)) {
            Some(ids) => ids.clone(),
            None => return vec![],
        };
        let mut sources = vec![];
        for eid in edge_ids {
            if let Some(edge) = g.edges.get(&eid) {
                match edge.from {
                    Endpoint::TransformOutput { transform: src, .. } => {
                        sources.push(src);
                    }
                    Endpoint::Io(io_n) => {
                        // Traverse back through IoNode: find the edge coming INTO io_n.
                        if let Some(io_node) = g.io_nodes.get(&io_n) {
                            if let Some(in_eid) = io_node.incoming {
                                if let Some(in_edge) = g.edges.get(&in_eid) {
                                    if let Endpoint::TransformOutput { transform: src, .. } = in_edge.from {
                                        sources.push(src);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        sources
    }

    /// Get last-known hash for a node's output (to detect if recompute changed anything).
    pub(crate) fn last_output_hash(&self, transform: NodeId, slot: usize) -> Option<ValueHash> {
        let g = self.inner.read().unwrap();
        let eids = g.transform_nodes.get(&transform)?.output_edges.get(slot)?;
        eids.first().and_then(|eid| {
            g.edges.get(eid).and_then(|e| match &e.payload {
                EdgePayload::Single(sv) => sv.hash,
                EdgePayload::Collection(_) => None,
            })
        })
    }

    /// Get the current element keys in a collection output slot.
    /// Used by fan-out transforms to detect removed elements.
    pub(crate) fn get_collection_output_keys(&self, transform: NodeId, slot: usize) -> Vec<u64> {
        let g = self.inner.read().unwrap();
        let eids = match g.transform_nodes.get(&transform)
            .and_then(|t| t.output_edges.get(slot)) {
            Some(ids) => ids.clone(),
            None => return vec![],
        };
        let mut keys = vec![];
        for eid in &eids {
            if let Some(edge) = g.edges.get(eid) {
                if let EdgePayload::Collection(elems) = &edge.payload {
                    keys.extend(elems.iter().map(|e| e.key));
                }
            }
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::{TransformSchema, TransformContext, TransformError, Transform};
    use async_trait::async_trait;

    struct DummyTransform;
    #[async_trait]
    impl crate::transform::Transform for DummyTransform {
        fn schema() -> TransformSchema where Self: Sized {
            TransformSchema::new().input::<u32>().output::<u32>()
        }
        async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
            let v = *ctx.input::<u32>(0)?;
            ctx.output(0, v + 1)
        }
    }

    fn make_erased() -> ErasedTransform {
        ErasedTransform::new(DummyTransform::schema(), DummyTransform)
    }

    #[test]
    fn add_nodes_and_edge() {
        let g = Graph::new();
        let a = NodeId::new();
        let b = NodeId::new();
        let t = NodeId::new();
        g.add_input_node(a).unwrap();
        g.add_output_node(b).unwrap();
        g.add_transform_node(t, "dummy".into(), make_erased()).unwrap();
        g.add_edge(Endpoint::Io(a), Endpoint::TransformInput { transform: t, slot: 0 }, false).unwrap();
        g.add_edge(Endpoint::TransformOutput { transform: t, slot: 0 }, Endpoint::Io(b), false).unwrap();
    }

    #[test]
    fn duplicate_node_rejected() {
        let g = Graph::new();
        let a = NodeId::new();
        g.add_input_node(a).unwrap();
        assert!(matches!(g.add_input_node(a), Err(GraphError::DuplicateNode(_))));
    }

    #[test]
    fn dirty_transform_after_set_input() {
        let g = Graph::new();
        let inp = NodeId::new();
        let t   = NodeId::new();
        g.add_input_node(inp).unwrap();
        g.add_transform_node(t, "d".into(), make_erased()).unwrap();
        g.add_edge(Endpoint::Io(inp), Endpoint::TransformInput { transform: t, slot: 0 }, false).unwrap();

        let reg = crate::value::ValueTypeRegistry::new();
        reg.register::<u32>().unwrap();
        let v = reg.make_value(42u32).unwrap();
        let h = crate::value::hash_value(&v, &reg);
        assert!(g.set_input(inp, v, h));

        let dirty = g.dirty_transforms_topo();
        assert!(dirty.contains(&t));
    }
}
