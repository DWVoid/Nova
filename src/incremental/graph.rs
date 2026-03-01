//! DAG topology, node/edge bookkeeping, and dirty-flag propagation.
//!
//! ## Design: Separating Topology from I/O
//!
//! `Graph` is a **pure in-memory data structure**.  It does not read from or
//! write to any storage backend.  All persistence is handled by
//! [`crate::incremental::loader::LazyLoader`].  This separation keeps the
//! graph's logic simple and fully synchronous, while storage operations
//! remain async.
//!
//! ## Design: `DashMap` for Lock-Free Concurrent Reads
//!
//! During parallel recomputation (see `scheduler.rs`), multiple Tokio tasks
//! read node entries concurrently.  Using a single `RwLock<HashMap>` would
//! serialise all reads onto a single lock.  [`dashmap::DashMap`] shards the
//! map into 64 independent buckets, so tasks working on different nodes
//! almost never contend.
//!
//! Writes (marking nodes dirty, storing new values) still require a shard
//! lock, but they are brief (no I/O).
//!
//! ## Design: Eager Dirty Propagation
//!
//! When an input changes, `mark_dirty` immediately BFS-propagates the dirty
//! flag forward through all transitive dependents.  This means the
//! recomputation phase only needs to read the dirty flag, not traverse the
//! graph again.  The trade-off is that `mark_dirty` may do redundant work if
//! many inputs change before the next `update()` call; in practice this is
//! negligible because the BFS terminates as soon as it reaches already-dirty
//! nodes.
//!
//! ## Design: `NodeStatus` for Error Nodes
//!
//! A failed transform leaves the node in `NodeStatus::Error` rather than
//! `NodeStatus::Dirty`.  This prevents the scheduler from retrying the same
//! failing transform in an infinite loop.  The error is cleared and the node
//! becomes `Dirty` again when any of its inputs change (via `mark_dirty`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use dashmap::DashMap;
use crate::incremental::node_id::NodeId;
use crate::incremental::transform::Transform;
use crate::incremental::value::{Value, ValueHash};
use crate::incremental::transform::TransformError;

// ---------------------------------------------------------------------------
// Edge identity
// ---------------------------------------------------------------------------

/// A local (non-persistent) identifier for a directed edge.
///
/// Edge IDs are assigned by an atomic counter and are **not** stable across
/// restarts.  They are used only as fast lookup keys within the in-memory
/// graph; the persistent representation uses source/target `NodeId` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeId(pub u64);

static NEXT_EDGE_ID: AtomicU64 = AtomicU64::new(1);
fn next_edge_id() -> EdgeId {
    EdgeId(NEXT_EDGE_ID.fetch_add(1, Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// Node status
// ---------------------------------------------------------------------------

/// The computational status of a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeStatus {
    /// The node's value is up to date.
    Clean,
    /// The node (or a dependency) has changed and needs recomputation.
    Dirty,
    /// The most recent attempt to compute this node failed.
    ///
    /// The node will be retried automatically once any input changes.
    Error(TransformError),
}

impl NodeStatus {
    pub fn is_dirty(&self) -> bool { matches!(self, NodeStatus::Dirty) }
    pub fn is_error(&self) -> bool { matches!(self, NodeStatus::Error(_)) }
}

// ---------------------------------------------------------------------------
// Node entry
// ---------------------------------------------------------------------------

/// All bookkeeping data for a single node.
#[derive(Debug)]
pub struct NodeEntry {
    pub id: NodeId,
    /// Current cached value.  `None` if never computed or evicted.
    pub value: Option<Value>,
    /// Hash of the value at last compute time.  Used for early-exit.
    pub value_hash: Option<ValueHash>,
    pub status: NodeStatus,
    /// `true` for nodes that are manually fed values (no incoming edges).
    pub is_input: bool,
    /// IDs of edges whose target list includes this node.
    pub incoming: Vec<EdgeId>,
    /// IDs of edges whose source list includes this node.
    pub outgoing: Vec<EdgeId>,
}

impl NodeEntry {
    pub fn new_input(id: NodeId) -> Self {
        Self {
            id,
            value: None,
            value_hash: None,
            status: NodeStatus::Dirty,
            is_input: true,
            incoming: Vec::new(),
            outgoing: Vec::new(),
        }
    }

    pub fn new_computed(id: NodeId) -> Self {
        Self {
            id,
            value: None,
            value_hash: None,
            status: NodeStatus::Dirty,
            is_input: false,
            incoming: Vec::new(),
            outgoing: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Edge entry
// ---------------------------------------------------------------------------

/// A directed edge connecting source nodes to target nodes via a transform.
#[derive(Clone, Debug)]
pub struct EdgeEntry {
    pub id: EdgeId,
    pub transform: Transform,
    /// Ordered input node IDs.
    pub sources: Vec<NodeId>,
    /// Ordered output node IDs.
    pub targets: Vec<NodeId>,
    /// User-assigned stable name for persistence/reload.
    pub transform_key: String,
}

// ---------------------------------------------------------------------------
// Graph errors
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

/// The in-memory incremental computation graph.
///
/// Holds node entries and edge entries in concurrent hash maps, and maintains
/// reverse-adjacency information for efficient dirty propagation.
///
/// Cloning a `Graph` gives a new handle that shares the same underlying maps,
/// similar to `Arc<T>`.  Use [`Graph::new`] to create an independent graph.
#[derive(Clone)]
pub struct Graph {
    nodes: Arc<DashMap<NodeId, NodeEntry>>,
    edges: Arc<DashMap<EdgeId, EdgeEntry>>,
}

impl Graph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(DashMap::new()),
            edges: Arc::new(DashMap::new()),
        }
    }

    // -----------------------------------------------------------------------
    // Node management
    // -----------------------------------------------------------------------

    /// Add a new input node (no incoming edges) and return its ID.
    ///
    /// Input nodes receive values via [`Graph::set_input`]; they are never
    /// computed by a transform.
    pub fn add_input_node(&self) -> NodeId {
        let id = NodeId::new();
        self.nodes.insert(id, NodeEntry::new_input(id));
        id
    }

    /// Add an input node with a pre-assigned `id`.
    ///
    /// Used when the caller derives a stable [`NodeId`] (e.g. via UUID v5
    /// from a file path) and needs the graph to use that exact identity.
    /// No-ops if a node with `id` already exists.
    pub fn add_input_node_with_id(&self, id: NodeId) {
        self.nodes.entry(id).or_insert_with(|| NodeEntry::new_input(id));
    }

    /// Add a new computed node (will have exactly one incoming edge) and
    /// return its ID.
    ///
    /// In most cases you do not call this directly; [`Graph::add_transform`]
    /// creates target nodes for you.
    pub fn add_computed_node(&self) -> NodeId {
        let id = NodeId::new();
        self.nodes.insert(id, NodeEntry::new_computed(id));
        id
    }

    /// Add a computed node with a pre-assigned `id`.
    ///
    /// No-ops if a node with `id` already exists.
    pub fn add_computed_node_with_id(&self, id: NodeId) {
        self.nodes.entry(id).or_insert_with(|| NodeEntry::new_computed(id));
    }

    /// Register a pre-existing node (loaded from storage) without
    /// overwriting it if it already exists.
    pub fn register_node(&self, entry: NodeEntry) {
        self.nodes.entry(entry.id).or_insert(entry);
    }

    /// Return `true` if a node with `id` exists in the graph.
    pub fn contains_node(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    // -----------------------------------------------------------------------
    // Edge management
    // -----------------------------------------------------------------------

    /// Connect `sources` to `targets` via `transform` and return the new
    /// [`EdgeId`].
    ///
    /// `transform_key` is a user-assigned stable string that survives
    /// serialisation (the actual transform function is not serialised).
    ///
    /// # Errors
    ///
    /// Returns an error if any source or target node does not exist in the
    /// graph.
    pub fn add_transform(
        &self,
        sources: Vec<NodeId>,
        targets: Vec<NodeId>,
        transform: Transform,
        transform_key: impl Into<String>,
    ) -> Result<EdgeId, GraphError> {
        for &s in &sources {
            if !self.nodes.contains_key(&s) {
                return Err(GraphError::new(format!("source node {s} not in graph")));
            }
        }
        for &t in &targets {
            if !self.nodes.contains_key(&t) {
                return Err(GraphError::new(format!("target node {t} not in graph")));
            }
        }

        let eid = next_edge_id();
        let entry = EdgeEntry {
            id: eid,
            transform,
            sources: sources.clone(),
            targets: targets.clone(),
            transform_key: transform_key.into(),
        };
        self.edges.insert(eid, entry);

        // Update reverse-adjacency on nodes.
        for &s in &sources {
            if let Some(mut n) = self.nodes.get_mut(&s) {
                n.outgoing.push(eid);
            }
        }
        for &t in &targets {
            if let Some(mut n) = self.nodes.get_mut(&t) {
                n.incoming.push(eid);
            }
        }

        Ok(eid)
    }

    /// Re-register a persisted edge (no adjacency update needed because nodes
    /// were loaded with their adjacency lists intact).
    pub fn register_edge(&self, entry: EdgeEntry) {
        self.edges.entry(entry.id).or_insert(entry);
    }

    // -----------------------------------------------------------------------
    // Input value management
    // -----------------------------------------------------------------------

    /// Set the value of an input node and mark it (and all transitive
    /// dependents) as dirty.
    ///
    /// Panics if `id` is not an input node.
    pub fn set_input(&self, id: NodeId, value: Value) -> Result<(), GraphError> {
        let mut node = self.nodes.get_mut(&id)
            .ok_or_else(|| GraphError::new(format!("node {id} not found")))?;
        if !node.is_input {
            return Err(GraphError::new(format!("node {id} is not an input node")));
        }
        node.value = Some(value);
        node.status = NodeStatus::Dirty;
        let id_copy = id;
        drop(node); // release shard lock before BFS
        self.propagate_dirty(id_copy);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Dirty tracking
    // -----------------------------------------------------------------------

    /// Mark `id` dirty and forward-propagate the dirty flag to all
    /// transitive dependents via BFS.
    ///
    /// Already-dirty nodes short-circuit the BFS to avoid redundant work.
    pub fn mark_dirty(&self, id: NodeId) {
        if let Some(mut n) = self.nodes.get_mut(&id) {
            n.status = NodeStatus::Dirty;
        }
        self.propagate_dirty(id);
    }

    /// BFS forward-propagation of the dirty flag starting from `start`.
    ///
    /// Does not re-mark `start` itself (the caller is responsible for that).
    ///
    /// Nodes in `NodeStatus::Error` are also re-marked `Dirty` so they are
    /// retried on the next `update()` call.  Nodes that are already `Dirty`
    /// short-circuit the BFS (their subtrees are already propagated).
    fn propagate_dirty(&self, start: NodeId) {
        let mut queue: VecDeque<NodeId> = VecDeque::new();
        // Enqueue direct successors.
        if let Some(node) = self.nodes.get(&start) {
            for &eid in &node.outgoing {
                if let Some(edge) = self.edges.get(&eid) {
                    for &target in &edge.targets {
                        queue.push_back(target);
                    }
                }
            }
        }

        let mut visited: HashSet<NodeId> = HashSet::new();
        while let Some(nid) = queue.pop_front() {
            if !visited.insert(nid) { continue; }
            if let Some(mut n) = self.nodes.get_mut(&nid) {
                if n.status.is_dirty() {
                    // Already dirty – subtree already propagated, skip.
                    continue;
                }
                // Mark both Clean and Error nodes dirty so they are retried.
                n.status = NodeStatus::Dirty;
                let outgoing = n.outgoing.clone();
                drop(n);
                for eid in outgoing {
                    if let Some(edge) = self.edges.get(&eid) {
                        for &t in &edge.targets {
                            queue.push_back(t);
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Scheduling helpers
    // -----------------------------------------------------------------------

    /// Return all dirty nodes in **topological order** (sources before
    /// dependents).
    ///
    /// Uses Kahn's algorithm over the subgraph induced by dirty nodes.
    ///
    /// Nodes in `NodeStatus::Error` are **not** included in the output (they
    /// are not re-evaluated until an input change re-marks them `Dirty`), but
    /// they are treated as ordering predecessors so that downstream `Dirty`
    /// nodes are not promoted to wave 0 and incorrectly evaluated before the
    /// error is resolved.
    pub fn dirty_nodes_topo(&self) -> Vec<NodeId> {
        // Collect all dirty node IDs.
        let dirty: HashSet<NodeId> = self.nodes.iter()
            .filter(|e| e.status.is_dirty())
            .map(|e| *e.key())
            .collect();

        if dirty.is_empty() { return vec![]; }

        // Also collect error nodes so they can act as in-degree contributors.
        let errored: HashSet<NodeId> = self.nodes.iter()
            .filter(|e| e.status.is_error())
            .map(|e| *e.key())
            .collect();

        // Build an in-degree map restricted to the dirty subgraph.
        // An edge contributes to in-degree if:
        //   - the target is dirty, AND
        //   - at least one source is dirty OR errored
        //     (meaning the target depends on something that is not yet clean)
        let mut in_degree: HashMap<NodeId, usize> = dirty.iter().map(|&id| (id, 0)).collect();
        let mut adj: HashMap<NodeId, Vec<NodeId>> = dirty.iter().map(|&id| (id, vec![])).collect();

        for entry in self.edges.iter() {
            let edge = entry.value();
            let targets_dirty = edge.targets.iter().any(|t| dirty.contains(t));
            if !targets_dirty { continue; }

            // A source contributes an ordering edge if it is dirty or errored.
            for &s in &edge.sources {
                if !dirty.contains(&s) && !errored.contains(&s) { continue; }
                for &t in &edge.targets {
                    if dirty.contains(&t) {
                        *in_degree.entry(t).or_default() += 1;
                        if dirty.contains(&s) {
                            // Only add real adjacency for dirty→dirty edges
                            // (errored nodes are excluded from the output).
                            adj.entry(s).or_default().push(t);
                        }
                    }
                }
            }
        }

        // Kahn's BFS topological sort over dirty nodes only.
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
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }
        result
    }

    /// Return the [`EdgeEntry`] whose targets include `node_id`, or `None` if
    /// the node is an input.
    pub fn incoming_edge_for(&self, node_id: NodeId) -> Option<EdgeEntry> {
        let node = self.nodes.get(&node_id)?;
        let eid = *node.incoming.first()?;
        self.edges.get(&eid).map(|e| e.clone())
    }

    /// Return all [`EdgeEntry`] objects in the graph (for persistence).
    pub fn all_edges(&self) -> Vec<EdgeEntry> {
        self.edges.iter().map(|e| e.value().clone()).collect()
    }

    /// Return all node IDs in the graph.
    pub fn all_node_ids(&self) -> Vec<NodeId> {
        self.nodes.iter().map(|e| *e.key()).collect()
    }

    // -----------------------------------------------------------------------
    // Value access (called by Scheduler / LazyLoader)
    // -----------------------------------------------------------------------

    /// Store a computed value and hash into a node entry and mark it clean.
    pub fn store_value(&self, id: NodeId, value: Value, hash: ValueHash) -> Result<(), GraphError> {
        let mut node = self.nodes.get_mut(&id)
            .ok_or_else(|| GraphError::new(format!("node {id} not found")))?;
        node.value = Some(value);
        node.value_hash = Some(hash);
        node.status = NodeStatus::Clean;
        Ok(())
    }

    /// Mark a node as errored.
    pub fn store_error(&self, id: NodeId, err: TransformError) -> Result<(), GraphError> {
        let mut node = self.nodes.get_mut(&id)
            .ok_or_else(|| GraphError::new(format!("node {id} not found")))?;
        node.status = NodeStatus::Error(err);
        Ok(())
    }

    /// Get a snapshot of the cached value and hash for `id`.
    pub fn peek_value(&self, id: NodeId) -> Option<(Value, ValueHash)> {
        let node = self.nodes.get(&id)?;
        let v = node.value.clone()?;
        let h = node.value_hash.unwrap_or(0);
        Some((v, h))
    }

    /// Get the current status of a node.
    pub fn node_status(&self, id: NodeId) -> Option<NodeStatus> {
        self.nodes.get(&id).map(|n| n.status.clone())
    }

    /// Check if a node is an input node.
    pub fn is_input(&self, id: NodeId) -> bool {
        self.nodes.get(&id).map(|n| n.is_input).unwrap_or(false)
    }

    /// Get a node's last known value hash (for change detection after reload).
    pub fn last_hash(&self, id: NodeId) -> Option<ValueHash> {
        self.nodes.get(&id)?.value_hash
    }

    /// Mark the direct successors of `id` dirty (not `id` itself).
    ///
    /// Called by the scheduler after a transform produces a new output so that
    /// downstream nodes are recomputed on the next wave or update cycle.
    pub fn mark_dirty_downstream(&self, id: NodeId) {
        if let Some(node) = self.nodes.get(&id) {
            let outgoing = node.outgoing.clone();
            drop(node);
            for eid in outgoing {
                if let Some(edge) = self.edges.get(&eid) {
                    let targets = edge.targets.clone();
                    drop(edge);
                    for t in targets {
                        self.mark_dirty(t);
                    }
                }
            }
        }
    }

    /// Get a mutable reference to a node entry (used during graph reload).
    pub fn nodes_mut(&self, id: NodeId) -> Option<dashmap::mapref::one::RefMut<'_, NodeId, NodeEntry>> {
        self.nodes.get_mut(&id)
    }

    // -----------------------------------------------------------------------
    // Node / edge removal
    // -----------------------------------------------------------------------

    /// Remove a node and all edges that touch it from the graph.
    ///
    /// For every edge that is removed, the peer nodes (sources or targets of
    /// that edge) have their adjacency lists updated so they no longer
    /// reference the deleted edge.  Downstream nodes that depended on this
    /// node are marked dirty so the next `update()` call knows to
    /// recompute them.
    ///
    /// Returns `true` if the node existed, `false` if it was not found.
    pub fn remove_node(&self, id: NodeId) -> bool {
        let entry = match self.nodes.remove(&id) {
            Some((_, e)) => e,
            None => return false,
        };

        // Collect all edge IDs that touched this node.
        let all_edge_ids: Vec<EdgeId> = entry.incoming.iter()
            .chain(entry.outgoing.iter())
            .copied()
            .collect();

        for eid in all_edge_ids {
            let edge = match self.edges.remove(&eid) {
                Some((_, e)) => e,
                None => continue,
            };

            // Remove this edge from peer nodes' adjacency lists.
            for &src in &edge.sources {
                if src == id { continue; }
                if let Some(mut n) = self.nodes.get_mut(&src) {
                    n.outgoing.retain(|e| *e != eid);
                }
            }
            for &tgt in &edge.targets {
                if tgt == id { continue; }
                if let Some(mut n) = self.nodes.get_mut(&tgt) {
                    n.incoming.retain(|e| *e != eid);
                    // Mark downstream nodes dirty: they were depending on this
                    // node and now have a missing input.
                    n.status = NodeStatus::Dirty;
                }
            }
        }
        true
    }
}

impl Default for Graph {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::transform::{Transform, OneToOneTransform};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct Identity;
    #[async_trait]
    impl OneToOneTransform for Identity {
        async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
            Ok(input.clone())
        }
    }

    fn make_transform() -> Transform {
        Transform::OneToOne(Arc::new(Identity))
    }

    #[test]
    fn add_input_node_creates_node() {
        let g = Graph::new();
        let id = g.add_input_node();
        assert!(g.contains_node(id));
        assert!(g.is_input(id));
    }

    #[test]
    fn add_transform_wires_adjacency() {
        let g = Graph::new();
        let src = g.add_input_node();
        let tgt = g.add_computed_node();
        let eid = g.add_transform(vec![src], vec![tgt], make_transform(), "id").unwrap();

        // Source should have outgoing edge, target should have incoming edge.
        let src_entry = g.nodes.get(&src).unwrap();
        assert!(src_entry.outgoing.contains(&eid));
        let tgt_entry = g.nodes.get(&tgt).unwrap();
        assert!(tgt_entry.incoming.contains(&eid));
    }

    #[test]
    fn add_transform_fails_for_unknown_node() {
        let g = Graph::new();
        let phantom = NodeId::new();
        let tgt = g.add_computed_node();
        let result = g.add_transform(vec![phantom], vec![tgt], make_transform(), "id");
        assert!(result.is_err());
    }

    #[test]
    fn dirty_propagates_forward() {
        let g = Graph::new();
        let a = g.add_input_node();
        let b = g.add_computed_node();
        let c = g.add_computed_node();
        g.add_transform(vec![a], vec![b], make_transform(), "ab").unwrap();
        g.add_transform(vec![b], vec![c], make_transform(), "bc").unwrap();

        // Initially all dirty (new nodes start dirty).
        // Mark b and c clean first.
        g.store_value(b, Value::new(0i32), 0).unwrap();
        g.store_value(c, Value::new(0i32), 0).unwrap();

        // Now set input on a → should propagate to b and c.
        g.set_input(a, Value::new(1i32)).unwrap();
        assert!(g.node_status(b).unwrap().is_dirty());
        assert!(g.node_status(c).unwrap().is_dirty());
    }

    #[test]
    fn dirty_nodes_topo_returns_sources_first() {
        let g = Graph::new();
        let a = g.add_input_node();
        let b = g.add_computed_node();
        g.add_transform(vec![a], vec![b], make_transform(), "ab").unwrap();

        let topo = g.dirty_nodes_topo();
        assert_eq!(topo.len(), 2);
        let pos_a = topo.iter().position(|&x| x == a).unwrap();
        let pos_b = topo.iter().position(|&x| x == b).unwrap();
        assert!(pos_a < pos_b, "source should appear before dependent");
    }

    #[test]
    fn store_value_clears_dirty() {
        let g = Graph::new();
        let a = g.add_input_node();
        g.store_value(a, Value::new(42i32), 99).unwrap();
        assert_eq!(g.node_status(a), Some(NodeStatus::Clean));
        let (v, h) = g.peek_value(a).unwrap();
        assert_eq!(v.downcast::<i32>(), Some(&42i32));
        assert_eq!(h, 99);
    }
}
