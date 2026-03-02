//! Bipartite incremental computation graph.
//!
//! ## Design: Transforms as Nodes, Values on Edges
//!
//! The graph is **bipartite**:
//! - [`InputNode`] / [`OutputNode`] are the user-visible endpoints.
//! - [`TransformNode`] nodes hold transform functions.
//! - [`ValueEdge`]s carry cached values between these node kinds.
//!
//! A `ValueEdge` can connect any of:
//! - `InputNode  → TransformNode` (input slot)
//! - `TransformNode → TransformNode` (transform-to-transform, slot wiring)
//! - `TransformNode → OutputNode` (output slot)
//! - `InputNode  → OutputNode` (passthrough, no transform)
//!
//! ## Design: Collection Edges
//!
//! A `Collection`-typed [`EdgePayload`] carries a [`CollectionEdge`] with
//! per-element dirty tracking.  Crossing-kind connections are handled by the
//! scheduler:
//! - `Collection output → Single input slot`: scheduler calls the transform
//!   once per element.
//! - `Single output → Collection input slot`: scheduler inserts the value
//!   into the collection using the slot's sorter.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use dashmap::DashMap;
use crate::collection::{CollectionEdge, CollectionDiff, CollectionElement};
use crate::cycle::{SccGroup, TarjanScc};
use crate::node_id::NodeId;
use crate::transform::{Transform, TransformError};
use crate::value::{Value, ValueHash};

// ---------------------------------------------------------------------------
// EdgeId
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EdgeId(pub u64);

static NEXT_EDGE_ID: AtomicU64 = AtomicU64::new(1);
fn next_edge_id() -> EdgeId {
    EdgeId(NEXT_EDGE_ID.fetch_add(1, Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// NodeStatus (for TransformNodes only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeStatus {
    Clean,
    Dirty,
    Error(TransformError),
}

impl NodeStatus {
    pub fn is_dirty(&self) -> bool { matches!(self, NodeStatus::Dirty) }
    pub fn is_error(&self) -> bool { matches!(self, NodeStatus::Error(_)) }
}

// ---------------------------------------------------------------------------
// Endpoint — one side of a ValueEdge
// ---------------------------------------------------------------------------

/// One side of a [`ValueEdge`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Endpoint {
    /// An `InputNode` or `OutputNode` (user-visible).
    Io(NodeId),
    /// An input slot of a `TransformNode`.
    TransformInput { transform: NodeId, slot: usize },
    /// An output slot of a `TransformNode`.
    TransformOutput { transform: NodeId, slot: usize },
}

impl Endpoint {
    pub fn node_id(&self) -> NodeId {
        match self {
            Endpoint::Io(id) => *id,
            Endpoint::TransformInput { transform, .. } => *transform,
            Endpoint::TransformOutput { transform, .. } => *transform,
        }
    }
}

// ---------------------------------------------------------------------------
// EdgePayload
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum EdgePayload {
    Single(SingleEdge),
    Collection(CollectionEdge),
}

#[derive(Clone, Debug, Default)]
pub struct SingleEdge {
    pub value:      Option<Value>,
    pub value_hash: Option<ValueHash>,
    pub dirty:      bool,
}

// ---------------------------------------------------------------------------
// ValueEdge
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ValueEdge {
    pub id:      EdgeId,
    pub from:    Endpoint,
    pub to:      Endpoint,
    pub payload: EdgePayload,
}

// ---------------------------------------------------------------------------
// IoNode (Input / Output)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoKind { Input, Output }

#[derive(Debug)]
pub struct IoNode {
    pub id:      NodeId,
    pub kind:    IoKind,
    /// Edge that writes to this node (None for InputNodes).
    pub incoming: Option<EdgeId>,
    /// Edges that read from this node (to transform input slots).
    pub outgoing: Vec<EdgeId>,
}

impl IoNode {
    pub fn new_input(id: NodeId) -> Self  { Self { id, kind: IoKind::Input,  incoming: None, outgoing: vec![] } }
    pub fn new_output(id: NodeId) -> Self { Self { id, kind: IoKind::Output, incoming: None, outgoing: vec![] } }
}

// ---------------------------------------------------------------------------
// TransformNode
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TransformNode {
    pub id:            NodeId,
    pub transform_key: String,
    pub transform:     Transform,
    /// input_edges[slot_idx] = list of EdgeIds feeding that slot.
    pub input_edges:  Vec<Vec<EdgeId>>,
    /// output_edges[slot_idx] = list of EdgeIds leaving that slot.
    pub output_edges: Vec<Vec<EdgeId>>,
    pub status:        NodeStatus,
}

impl TransformNode {
    pub fn new(id: NodeId, transform_key: String, transform: Transform) -> Self {
        let n_in  = transform.schema().inputs.len();
        let n_out = transform.schema().outputs.len();
        Self {
            id,
            transform_key,
            transform,
            input_edges:  vec![vec![]; n_in],
            output_edges: vec![vec![]; n_out],
            status:        NodeStatus::Dirty,
        }
    }
}

// ---------------------------------------------------------------------------
// GraphError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GraphError {
    pub message: String,
}

impl GraphError {
    pub fn new(msg: impl Into<String>) -> Self { Self { message: msg.into() } }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for GraphError {}

// ---------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------

/// The bipartite incremental computation graph.
#[derive(Clone)]
pub struct Graph {
    io_nodes:        Arc<DashMap<NodeId, IoNode>>,
    transform_nodes: Arc<DashMap<NodeId, TransformNode>>,
    edges:           Arc<DashMap<EdgeId, ValueEdge>>,
    /// Legal SCC groups (cycles through Collection input slots).
    scc_groups:      Arc<parking_lot::RwLock<Vec<SccGroup>>>,
}

impl Graph {
    pub fn new() -> Self {
        Self {
            io_nodes:        Arc::new(DashMap::new()),
            transform_nodes: Arc::new(DashMap::new()),
            edges:           Arc::new(DashMap::new()),
            scc_groups:      Arc::new(parking_lot::RwLock::new(Vec::new())),
        }
    }

    // -----------------------------------------------------------------------
    // Node management
    // -----------------------------------------------------------------------

    pub fn add_input_node(&self) -> NodeId {
        let id = NodeId::new();
        self.io_nodes.insert(id, IoNode::new_input(id));
        id
    }

    pub fn add_input_node_with_id(&self, id: NodeId) {
        self.io_nodes.entry(id).or_insert_with(|| IoNode::new_input(id));
    }

    pub fn add_output_node(&self) -> NodeId {
        let id = NodeId::new();
        self.io_nodes.insert(id, IoNode::new_output(id));
        id
    }

    pub fn add_output_node_with_id(&self, id: NodeId) {
        self.io_nodes.entry(id).or_insert_with(|| IoNode::new_output(id));
    }

    /// Backward-compat alias for `add_output_node_with_id`.
    pub fn add_computed_node_with_id(&self, id: NodeId) {
        self.add_output_node_with_id(id);
    }

    pub fn add_transform_node(&self, key: impl Into<String>, transform: Transform) -> NodeId {
        let id = NodeId::new();
        let tn = TransformNode::new(id, key.into(), transform);
        self.transform_nodes.insert(id, tn);
        id
    }

    pub fn add_transform_node_with_id(&self, id: NodeId, key: impl Into<String>, transform: Transform) {
        self.transform_nodes.entry(id).or_insert_with(|| TransformNode::new(id, key.into(), transform));
    }

    pub fn contains_io_node(&self, id: NodeId) -> bool {
        self.io_nodes.contains_key(&id)
    }

    pub fn contains_transform_node(&self, id: NodeId) -> bool {
        self.transform_nodes.contains_key(&id)
    }

    pub fn contains_node(&self, id: NodeId) -> bool {
        self.io_nodes.contains_key(&id) || self.transform_nodes.contains_key(&id)
    }

    pub fn is_input_node(&self, id: NodeId) -> bool {
        self.io_nodes.get(&id).map(|n| n.kind == IoKind::Input).unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Edge management (wiring)
    // -----------------------------------------------------------------------

    /// Add a Single-payload edge from `from` to `to`.
    pub fn add_single_edge(&self, from: Endpoint, to: Endpoint) -> Result<EdgeId, GraphError> {
        self.validate_endpoints(&from, &to)?;
        let eid = next_edge_id();
        let edge = ValueEdge {
            id: eid,
            from: from.clone(),
            to: to.clone(),
            payload: EdgePayload::Single(SingleEdge::default()),
        };
        self.edges.insert(eid, edge);
        self.register_edge_in_nodes(eid, &from, &to);
        Ok(eid)
    }

    /// Add a Collection-payload edge from `from` to `to`.
    pub fn add_collection_edge(&self, from: Endpoint, to: Endpoint) -> Result<EdgeId, GraphError> {
        self.validate_endpoints(&from, &to)?;
        let eid = next_edge_id();
        let edge = ValueEdge {
            id: eid,
            from: from.clone(),
            to: to.clone(),
            payload: EdgePayload::Collection(CollectionEdge::new()),
        };
        self.edges.insert(eid, edge);
        self.register_edge_in_nodes(eid, &from, &to);
        Ok(eid)
    }

    fn validate_endpoints(&self, from: &Endpoint, to: &Endpoint) -> Result<(), GraphError> {
        self.check_endpoint_exists(from)?;
        self.check_endpoint_exists(to)?;
        Ok(())
    }

    fn check_endpoint_exists(&self, ep: &Endpoint) -> Result<(), GraphError> {
        match ep {
            Endpoint::Io(id) => {
                if !self.io_nodes.contains_key(id) {
                    return Err(GraphError::new(format!("IoNode {id} not in graph")));
                }
            }
            Endpoint::TransformInput { transform, slot } |
            Endpoint::TransformOutput { transform, slot } => {
                let tn = self.transform_nodes.get(transform)
                    .ok_or_else(|| GraphError::new(format!("TransformNode {transform} not in graph")))?;
                let count = match ep {
                    Endpoint::TransformInput  { .. } => tn.input_edges.len(),
                    Endpoint::TransformOutput { .. } => tn.output_edges.len(),
                    _ => unreachable!(),
                };
                if *slot >= count {
                    return Err(GraphError::new(format!(
                        "slot {slot} out of range (transform {transform} has {count} slots)"
                    )));
                }
            }
        }
        Ok(())
    }

    fn register_edge_in_nodes(&self, eid: EdgeId, from: &Endpoint, to: &Endpoint) {
        // "from" side: register as outgoing.
        match from {
            Endpoint::Io(id) => {
                if let Some(mut n) = self.io_nodes.get_mut(id) { n.outgoing.push(eid); }
            }
            Endpoint::TransformOutput { transform, slot } => {
                if let Some(mut t) = self.transform_nodes.get_mut(transform) {
                    if *slot < t.output_edges.len() {
                        t.output_edges[*slot].push(eid);
                    }
                }
            }
            _ => {}
        }
        // "to" side: register as incoming.
        match to {
            Endpoint::Io(id) => {
                if let Some(mut n) = self.io_nodes.get_mut(id) { n.incoming = Some(eid); }
            }
            Endpoint::TransformInput { transform, slot } => {
                if let Some(mut t) = self.transform_nodes.get_mut(transform) {
                    if *slot < t.input_edges.len() {
                        t.input_edges[*slot].push(eid);
                    }
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Input value management
    // -----------------------------------------------------------------------

    /// Set the value of an InputNode and mark all downstream transforms dirty.
    pub fn set_input(&self, id: NodeId, value: Value, hash: ValueHash) -> Result<(), GraphError> {
        let node = self.io_nodes.get(&id)
            .ok_or_else(|| GraphError::new(format!("IoNode {id} not found")))?;
        if node.kind != IoKind::Input {
            return Err(GraphError::new(format!("node {id} is not an input node")));
        }
        let outgoing = node.outgoing.clone();
        drop(node);
        // Store value on all outgoing single edges.
        for eid in &outgoing {
            if let Some(mut edge) = self.edges.get_mut(eid) {
                match &mut edge.payload {
                    EdgePayload::Single(s) => {
                        s.value = Some(value.clone());
                        s.value_hash = Some(hash);
                        s.dirty = true;
                    }
                    EdgePayload::Collection(c) => {
                        // Single input feeding a collection: insert/update element.
                        let key = hash; // content hash = element key
                        Self::upsert_collection_element(c, key, value.clone(), hash);
                    }
                }
            }
        }
        self.propagate_dirty_from_io(id);
        Ok(())
    }

    fn upsert_collection_element(c: &mut CollectionEdge, key: crate::collection::ElementKey, value: Value, hash: ValueHash) {
        match c.find_by_key(key) {
            Ok(pos) => {
                c.elements[pos].value = value;
                c.elements[pos].hash = hash;
                c.elements[pos].dirty = true;
            }
            Err(pos) => {
                c.elements.insert(pos, CollectionElement {
                    key, value, hash, dirty: true, nested: None,
                });
            }
        }
        c.dirty = true;
        c.recompute_full_hash();
    }

    // -----------------------------------------------------------------------
    // Dirty propagation
    // -----------------------------------------------------------------------

    /// Propagate dirty from an IoNode to downstream TransformNodes.
    pub fn propagate_dirty_from_io(&self, io_id: NodeId) {
        if let Some(node) = self.io_nodes.get(&io_id) {
            let outgoing = node.outgoing.clone();
            drop(node);
            for eid in outgoing {
                if let Some(edge) = self.edges.get(&eid) {
                    let to = edge.to.clone();
                    drop(edge);
                    self.mark_transform_dirty_from_endpoint(&to);
                }
            }
        }
    }

    fn mark_transform_dirty_from_endpoint(&self, ep: &Endpoint) {
        if let Endpoint::TransformInput { transform, .. } | Endpoint::TransformOutput { transform, .. } = ep {
            self.mark_transform_dirty(*transform);
        }
    }

    /// Mark a TransformNode dirty and propagate forward through its output edges.
    pub fn mark_transform_dirty(&self, id: NodeId) {
        if let Some(mut tn) = self.transform_nodes.get_mut(&id) {
            if tn.status.is_dirty() { return; } // already dirty, subtree already propagated
            tn.status = NodeStatus::Dirty;
            let output_edges: Vec<Vec<EdgeId>> = tn.output_edges.clone();
            drop(tn);
            for slot_edges in output_edges {
                for eid in slot_edges {
                    if let Some(mut edge) = self.edges.get_mut(&eid) {
                        match &mut edge.payload {
                            EdgePayload::Single(s) => s.dirty = true,
                            EdgePayload::Collection(c) => c.dirty = true,
                        }
                        let to = edge.to.clone();
                        drop(edge);
                        // propagate downstream
                        match &to {
                            Endpoint::Io(io_id) => {
                                // downstream IoNode → mark its outgoing edges' transforms dirty
                                self.propagate_dirty_from_io(*io_id);
                            }
                            Endpoint::TransformInput { transform, .. } => {
                                self.mark_transform_dirty(*transform);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Dirty nodes (transforms) in topological order
    // -----------------------------------------------------------------------

    /// Return all dirty `TransformNode` IDs in topological order (inputs first).
    pub fn dirty_transforms_topo(&self) -> Vec<NodeId> {
        let dirty: HashSet<NodeId> = self.transform_nodes.iter()
            .filter(|e| e.status.is_dirty())
            .map(|e| *e.key())
            .collect();
        if dirty.is_empty() { return vec![]; }

        let errored: HashSet<NodeId> = self.transform_nodes.iter()
            .filter(|e| e.status.is_error())
            .map(|e| *e.key())
            .collect();

        // Build in-degree map over dirty transforms.
        // T depends on T' if any input edge of T has its source as an output
        // edge of T'.
        let mut in_degree: HashMap<NodeId, usize> = dirty.iter().map(|&id| (id, 0)).collect();
        let mut adj: HashMap<NodeId, Vec<NodeId>> = dirty.iter().map(|&id| (id, vec![])).collect();

        for t_ref in self.transform_nodes.iter() {
            let tid = *t_ref.key();
            if !dirty.contains(&tid) { continue; }
            for slot_edges in &t_ref.input_edges {
                for &eid in slot_edges {
                    if let Some(edge) = self.edges.get(&eid) {
                        let upstream = self.upstream_transform_of_edge(&edge.from);
                        for u in upstream {
                            if dirty.contains(&u) || errored.contains(&u) {
                                *in_degree.entry(tid).or_default() += 1;
                                if dirty.contains(&u) {
                                    adj.entry(u).or_default().push(tid);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Kahn's BFS
        let mut queue: VecDeque<NodeId> = in_degree.iter()
            .filter(|(_, d)| **d == 0)
            .map(|(&id, _)| id)
            .collect();
        let mut result = Vec::with_capacity(dirty.len());
        while let Some(nid) = queue.pop_front() {
            result.push(nid);
            if let Some(neighbours) = adj.get(&nid) {
                for &next in neighbours {
                    let deg = in_degree.entry(next).or_default();
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 { queue.push_back(next); }
                }
            }
        }
        result
    }

    /// Given an `Endpoint` on the "from" side of an edge, find which
    /// `TransformNode`(s) produced that edge's value.
    fn upstream_transform_of_edge(&self, from: &Endpoint) -> Vec<NodeId> {
        match from {
            Endpoint::TransformOutput { transform, .. } => vec![*transform],
            Endpoint::Io(io_id) => {
                // The IoNode might be fed by a TransformNode output edge.
                if let Some(node) = self.io_nodes.get(io_id) {
                    if let Some(eid) = node.incoming {
                        if let Some(edge) = self.edges.get(&eid) {
                            return self.upstream_transform_of_edge(&edge.from.clone());
                        }
                    }
                }
                vec![]
            }
            _ => vec![],
        }
    }

    // -----------------------------------------------------------------------
    // Edge value access (called by Scheduler)
    // -----------------------------------------------------------------------

    /// Store a computed value on a Single edge and mark it clean.
    pub fn store_single_value(&self, eid: EdgeId, value: Value, hash: ValueHash) -> Result<(), GraphError> {
        let mut edge = self.edges.get_mut(&eid)
            .ok_or_else(|| GraphError::new(format!("edge {eid:?} not found")))?;
        match &mut edge.payload {
            EdgePayload::Single(s) => {
                s.value = Some(value);
                s.value_hash = Some(hash);
                s.dirty = false;
            }
            EdgePayload::Collection(_) => {
                return Err(GraphError::new("store_single_value called on a Collection edge"));
            }
        }
        Ok(())
    }

    /// Update collection state on a Collection edge (from scheduler after transform run).
    pub fn store_collection_diff(
        &self,
        eid: EdgeId,
        new_elements: Vec<(crate::collection::ElementKey, Value, ValueHash)>,
        sorter: &(dyn Fn(&Value, &Value) -> std::cmp::Ordering + Send + Sync),
    ) -> Result<CollectionDiff, GraphError> {
        let mut edge = self.edges.get_mut(&eid)
            .ok_or_else(|| GraphError::new(format!("edge {eid:?} not found")))?;
        match &mut edge.payload {
            EdgePayload::Collection(c) => {
                let old = c.elements.clone();
                // Replace elements with new ones.
                c.elements = new_elements.into_iter().map(|(key, value, hash)| {
                    CollectionElement { key, value, hash, dirty: true, nested: None }
                }).collect();
                // Re-sort.
                c.elements.sort_by(|a, b| sorter(&a.value, &b.value));
                // Rebuild keys from hash.
                for el in &mut c.elements {
                    el.key = el.hash;
                }
                c.recompute_full_hash();
                c.dirty = !c.elements.is_empty();
                let diff = CollectionDiff::compute(&old, &c.elements);
                Ok(diff)
            }
            EdgePayload::Single(_) => Err(GraphError::new("store_collection_diff on Single edge")),
        }
    }

    /// Peek the single value and hash on an edge.
    pub fn peek_single_value(&self, eid: EdgeId) -> Option<(Value, ValueHash)> {
        let edge = self.edges.get(&eid)?;
        match &edge.payload {
            EdgePayload::Single(s) => {
                let v = s.value.clone()?;
                let h = s.value_hash.unwrap_or(0);
                Some((v, h))
            }
            _ => None,
        }
    }

    /// Peek the single value of a specific IoNode by reading its incoming edge.
    pub fn peek_io_value(&self, io_id: NodeId) -> Option<(Value, ValueHash)> {
        let node = self.io_nodes.get(&io_id)?;
        match node.kind {
            IoKind::Input => {
                // For input nodes, check all outgoing single edges for a value.
                let outgoing = node.outgoing.clone();
                drop(node);
                for eid in outgoing {
                    if let Some(v) = self.peek_single_value(eid) { return Some(v); }
                }
                None
            }
            IoKind::Output => {
                let incoming = node.incoming?;
                drop(node);
                self.peek_single_value(incoming)
            }
        }
    }

    /// Get the last-known hash for the value on an edge.
    pub fn edge_hash(&self, eid: EdgeId) -> Option<ValueHash> {
        let edge = self.edges.get(&eid)?;
        match &edge.payload {
            EdgePayload::Single(s) => s.value_hash,
            EdgePayload::Collection(c) => Some(c.full_hash),
        }
    }

    /// Mark a TransformNode as errored.
    pub fn store_transform_error(&self, id: NodeId, err: TransformError) -> Result<(), GraphError> {
        let mut tn = self.transform_nodes.get_mut(&id)
            .ok_or_else(|| GraphError::new(format!("TransformNode {id} not found")))?;
        tn.status = NodeStatus::Error(err);
        Ok(())
    }

    /// Mark a TransformNode as clean.
    pub fn mark_transform_clean(&self, id: NodeId) {
        if let Some(mut tn) = self.transform_nodes.get_mut(&id) {
            tn.status = NodeStatus::Clean;
        }
    }

    /// Get the status of a TransformNode.
    pub fn transform_status(&self, id: NodeId) -> Option<NodeStatus> {
        self.transform_nodes.get(&id).map(|t| t.status.clone())
    }

    /// Get the effective status of any node (backward compat).
    ///
    /// For `TransformNode`s: returns the node's own status.
    /// For `IoNode`s: infers status from the incoming edge's dirty flag and
    /// the upstream transform's error/dirty state.
    /// Returns `None` if the node does not exist.
    pub fn node_status(&self, id: NodeId) -> Option<NodeStatus> {
        // Try TransformNode first.
        if let Some(tn) = self.transform_nodes.get(&id) {
            return Some(tn.status.clone());
        }
        // For IoNodes, infer from incoming edge and upstream transform.
        if let Some(node) = self.io_nodes.get(&id) {
            if let Some(eid) = node.incoming {
                drop(node);
                if let Some(edge) = self.edges.get(&eid) {
                    // Check upstream transform status.
                    if let Endpoint::TransformOutput { transform, .. } = &edge.from {
                        if let Some(tn) = self.transform_nodes.get(transform) {
                            match tn.status {
                                NodeStatus::Error(_) => return Some(tn.status.clone()),
                                NodeStatus::Dirty    => return Some(NodeStatus::Dirty),
                                NodeStatus::Clean    => {}
                            }
                        }
                    }
                    let dirty = match &edge.payload {
                        EdgePayload::Single(s) => s.dirty,
                        EdgePayload::Collection(c) => c.dirty,
                    };
                    return Some(if dirty { NodeStatus::Dirty } else { NodeStatus::Clean });
                }
            } else {
                drop(node);
            }
            return Some(NodeStatus::Clean);
        }
        None
    }

    // -----------------------------------------------------------------------
    // SCC management
    // -----------------------------------------------------------------------

    /// Recompute SCCs over the TransformNode graph and store legal ones.
    /// Returns an error if an illegal cycle is detected.
    pub fn recompute_sccs(&self) -> Result<(), GraphError> {
        // Build adjacency: T → [downstream TransformNodes via output edges].
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for t_ref in self.transform_nodes.iter() {
            let tid = *t_ref.key();
            adj.entry(tid).or_default();
            for slot_edges in &t_ref.output_edges {
                for &eid in slot_edges {
                    if let Some(edge) = self.edges.get(&eid) {
                        for downstream in self.downstream_transforms_of_edge(&edge.to) {
                            adj.entry(tid).or_default().push(downstream);
                        }
                    }
                }
            }
        }
        let all_ids: Vec<NodeId> = adj.keys().copied().collect();
        let sccs = TarjanScc::new(&adj).run(all_ids);
        let mut legal_sccs = Vec::new();
        for scc in sccs {
            if scc.len() <= 1 {
                // Check self-loop.
                let tid = scc[0];
                let is_self_loop = adj.get(&tid).map(|n| n.contains(&tid)).unwrap_or(false);
                if !is_self_loop { continue; }
                // Self-loop: validate back-edge targets Collection slot.
                self.validate_scc_back_edges(&scc)?;
                legal_sccs.push(SccGroup::new(scc));
            } else {
                self.validate_scc_back_edges(&scc)?;
                legal_sccs.push(SccGroup::new(scc));
            }
        }
        *self.scc_groups.write() = legal_sccs;
        Ok(())
    }

    /// For each back-edge in the SCC, verify it targets a Collection input slot.
    fn validate_scc_back_edges(&self, scc: &[NodeId]) -> Result<(), GraphError> {
        let scc_set: HashSet<NodeId> = scc.iter().copied().collect();
        for &tid in scc {
            if let Some(tn) = self.transform_nodes.get(&tid) {
                for (slot_idx, slot_edges) in tn.input_edges.iter().enumerate() {
                    for &eid in slot_edges {
                        if let Some(edge) = self.edges.get(&eid) {
                            let upstream = self.upstream_transform_of_edge(&edge.from);
                            for u in upstream {
                                if scc_set.contains(&u) {
                                    // This is a back-edge. Verify slot is Collection.
                                    let slot_kind = &tn.transform.schema().inputs[slot_idx].kind;
                                    if !slot_kind.is_collection() {
                                        return Err(GraphError::new(format!(
                                            "illegal cycle: back-edge into TransformNode {tid} \
                                             slot {slot_idx} targets a Single slot (only Collection \
                                             slots may form cycles)"
                                        )));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn downstream_transforms_of_edge(&self, to: &Endpoint) -> Vec<NodeId> {
        match to {
            Endpoint::TransformInput { transform, .. } => vec![*transform],
            Endpoint::Io(io_id) => {
                if let Some(node) = self.io_nodes.get(io_id) {
                    let outgoing = node.outgoing.clone();
                    drop(node);
                    outgoing.into_iter().flat_map(|eid| {
                        if let Some(edge) = self.edges.get(&eid) {
                            self.downstream_transforms_of_edge(&edge.to.clone())
                        } else { vec![] }
                    }).collect()
                } else { vec![] }
            }
            _ => vec![],
        }
    }

    /// Return a snapshot of the current SCC groups.
    pub fn scc_groups(&self) -> Vec<SccGroup> {
        self.scc_groups.read().clone()
    }

    // -----------------------------------------------------------------------
    // Accessors for scheduler / loader
    // -----------------------------------------------------------------------

    pub fn transform_node_ids(&self) -> Vec<NodeId> {
        self.transform_nodes.iter().map(|e| *e.key()).collect()
    }

    pub fn io_node_ids(&self) -> Vec<NodeId> {
        self.io_nodes.iter().map(|e| *e.key()).collect()
    }

    pub fn edge_ids(&self) -> Vec<EdgeId> {
        self.edges.iter().map(|e| *e.key()).collect()
    }

    /// Return a clone of a TransformNode's full data (for scheduler).
    pub fn get_transform_node(&self, id: NodeId) -> Option<dashmap::mapref::one::Ref<'_, NodeId, TransformNode>> {
        self.transform_nodes.get(&id)
    }

    pub fn get_transform_node_mut(&self, id: NodeId) -> Option<dashmap::mapref::one::RefMut<'_, NodeId, TransformNode>> {
        self.transform_nodes.get_mut(&id)
    }

    pub fn get_io_node(&self, id: NodeId) -> Option<dashmap::mapref::one::Ref<'_, NodeId, IoNode>> {
        self.io_nodes.get(&id)
    }

    pub fn get_edge(&self, eid: EdgeId) -> Option<dashmap::mapref::one::Ref<'_, EdgeId, ValueEdge>> {
        self.edges.get(&eid)
    }

    pub fn get_edge_mut(&self, eid: EdgeId) -> Option<dashmap::mapref::one::RefMut<'_, EdgeId, ValueEdge>> {
        self.edges.get_mut(&eid)
    }

    // -----------------------------------------------------------------------
    // Node removal
    // -----------------------------------------------------------------------

    pub fn remove_io_node(&self, id: NodeId) -> bool {
        let node = match self.io_nodes.remove(&id) {
            Some((_, n)) => n,
            None => return false,
        };
        // Propagate dirty to downstream transforms before removing edges.
        for &eid in &node.outgoing {
            if let Some(edge) = self.edges.get(&eid) {
                match &edge.to {
                    Endpoint::TransformInput { transform, .. } => {
                        self.mark_transform_dirty(*transform);
                    }
                    _ => {}
                }
            }
        }
        for &eid in &node.outgoing {
            self.edges.remove(&eid);
        }
        if let Some(eid) = node.incoming {
            self.edges.remove(&eid);
        }
        true
    }

    pub fn remove_transform_node(&self, id: NodeId) -> bool {
        let node = match self.transform_nodes.remove(&id) {
            Some((_, n)) => n,
            None => return false,
        };
        // Propagate dirty to downstream nodes via output edges.
        for slot_edges in &node.output_edges {
            for &eid in slot_edges {
                if let Some(edge) = self.edges.get(&eid) {
                    match &edge.to {
                        Endpoint::Io(io_id) => { self.propagate_dirty_from_io(*io_id); }
                        Endpoint::TransformInput { transform, .. } => {
                            self.mark_transform_dirty(*transform);
                        }
                        _ => {}
                    }
                }
            }
        }
        for slot_edges in node.input_edges.iter().chain(node.output_edges.iter()) {
            for &eid in slot_edges {
                self.edges.remove(&eid);
            }
        }
        // Invalidate SCC cache.
        self.scc_groups.write().clear();
        true
    }
}

impl Default for Graph {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::{Transform, TypedOneToOne};
    use crate::value::{Value, ValueTypeRegistry};
    use crate::slot::TransformSchema;
    use std::sync::Arc;

    fn make_registry() -> Arc<ValueTypeRegistry> {
        let mut r = ValueTypeRegistry::new();
        r.register_primitives().unwrap();
        Arc::new(r)
    }

    fn make_double_transform(reg: Arc<ValueTypeRegistry>) -> Transform {
        Transform::new(Arc::new(TypedOneToOne::new(
            |n: &i32| { let n = *n; async move { Ok(n * 2) } },
            reg,
        )))
    }

    fn val(reg: &Arc<ValueTypeRegistry>, n: i32) -> (Value, ValueHash) {
        let v = reg.make_value(n).unwrap();
        let h = crate::value::hash_bytes(&reg.serialize_value(&v));
        (v, h)
    }

    #[test]
    fn add_and_connect_nodes() {
        let g = Graph::new();
        let reg = make_registry();
        let input  = g.add_input_node();
        let output = g.add_output_node();
        let t      = g.add_transform_node("double", make_double_transform(reg));
        let e1 = g.add_single_edge(
            Endpoint::Io(input),
            Endpoint::TransformInput { transform: t, slot: 0 },
        ).unwrap();
        let e2 = g.add_single_edge(
            Endpoint::TransformOutput { transform: t, slot: 0 },
            Endpoint::Io(output),
        ).unwrap();
        assert!(g.edges.contains_key(&e1));
        assert!(g.edges.contains_key(&e2));
    }

    #[test]
    fn set_input_marks_transform_dirty() {
        let g = Graph::new();
        let reg = make_registry();
        let input  = g.add_input_node();
        let t      = g.add_transform_node("double", make_double_transform(reg.clone()));
        g.add_single_edge(
            Endpoint::Io(input),
            Endpoint::TransformInput { transform: t, slot: 0 },
        ).unwrap();
        g.mark_transform_clean(t);
        let (v, h) = val(&reg, 5);
        g.set_input(input, v, h).unwrap();
        assert!(g.transform_status(t).unwrap().is_dirty());
    }

    #[test]
    fn dirty_topo_returns_correct_order() {
        let g = Graph::new();
        let reg = make_registry();
        let a = g.add_input_node();
        let t1 = g.add_transform_node("t1", make_double_transform(reg.clone()));
        let t2 = g.add_transform_node("t2", make_double_transform(reg.clone()));
        let out = g.add_output_node();
        g.add_single_edge(Endpoint::Io(a), Endpoint::TransformInput { transform: t1, slot: 0 }).unwrap();
        g.add_single_edge(
            Endpoint::TransformOutput { transform: t1, slot: 0 },
            Endpoint::TransformInput { transform: t2, slot: 0 },
        ).unwrap();
        g.add_single_edge(Endpoint::TransformOutput { transform: t2, slot: 0 }, Endpoint::Io(out)).unwrap();

        let topo = g.dirty_transforms_topo();
        assert_eq!(topo.len(), 2);
        let pos_t1 = topo.iter().position(|&x| x == t1).unwrap();
        let pos_t2 = topo.iter().position(|&x| x == t2).unwrap();
        assert!(pos_t1 < pos_t2, "t1 must come before t2");
    }
}
