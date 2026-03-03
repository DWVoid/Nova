//! Static graph topology — frozen after `EngineBuilder::build()`.
//!
//! ## Design
//!
//! [`Topology`] holds only immutable structural data: which nodes exist, which
//! edges connect them, and the pre-computed topological order.  After `build()`
//! it is wrapped in `Arc<Topology>` and shared freely across threads with no
//! locking overhead on any read path.
//!
//! [`WorkState`](crate::workstate::WorkState) holds the mutable per-cycle
//! state (values, dirty flags, node status).

use std::collections::HashMap;
use std::sync::Arc;
use crate::node_id::NodeId;
use crate::transform::{ErasedTransform, SlotLayout, DispatchTable};
use crate::value::ValueTypeRegistry;

// ---------------------------------------------------------------------------
// EdgeId
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
// TopoEdge — static edge metadata (no value)
// ---------------------------------------------------------------------------

/// Static description of one edge in the topology.
#[derive(Debug, Clone)]
pub(crate) struct TopoEdge {
    pub(crate) from:        Endpoint,
    pub(crate) to:          Endpoint,
    /// True if this edge carries a collection (Vec<Element>), false for a single value.
    pub(crate) is_collection: bool,
}

// ---------------------------------------------------------------------------
// IoAdjacency
// ---------------------------------------------------------------------------

/// Static adjacency for one IoNode.
#[derive(Debug, Clone, Default)]
pub(crate) struct IoAdjacency {
    /// The single incoming edge (from a transform output or absent for input nodes).
    pub(crate) incoming: Option<EdgeId>,
    /// All outgoing edges (to transform input slots or to other IoNodes).
    pub(crate) outgoing: Vec<EdgeId>,
}

// ---------------------------------------------------------------------------
// TransformMeta
// ---------------------------------------------------------------------------

/// Static metadata for one transform node.
#[derive(Debug, Clone)]
pub(crate) struct TransformMeta {
    pub(crate) erased:       ErasedTransform,
    /// `input_edges[slot]` = list of EdgeIds feeding into that input slot.
    pub(crate) input_edges:  Vec<Vec<EdgeId>>,
    /// `output_edges[slot]` = list of EdgeIds carrying that output slot's value.
    pub(crate) output_edges: Vec<Vec<EdgeId>>,
}

impl TransformMeta {
    pub(crate) fn layout(&self) -> &Arc<SlotLayout>   { &self.erased.schema   }
    pub(crate) fn dispatch(&self) -> &Arc<DispatchTable> { &self.erased.dispatch }
}

// ---------------------------------------------------------------------------
// TopologyError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) enum TopologyError {
    DuplicateNode(NodeId),
    NodeNotFound(NodeId),
    SlotOutOfRange { node: NodeId, slot: usize },
    DuplicateEdge  { from: Endpoint, to: Endpoint },
    TypeConflict(String),
}

impl std::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TopologyError::DuplicateNode(id)         => write!(f, "duplicate node {id}"),
            TopologyError::NodeNotFound(id)          => write!(f, "node {id} not found"),
            TopologyError::SlotOutOfRange { node, slot } =>
                write!(f, "slot {slot} out of range on node {node}"),
            TopologyError::DuplicateEdge { from, to } =>
                write!(f, "duplicate edge {from:?} → {to:?}"),
            TopologyError::TypeConflict(msg)         => write!(f, "type conflict: {msg}"),
        }
    }
}
impl std::error::Error for TopologyError {}

// ---------------------------------------------------------------------------
// TopologyBuilder
// ---------------------------------------------------------------------------

/// Mutable accumulator for building a [`Topology`].
/// Used exclusively by `EngineBuilder::build()`.
pub(crate) struct TopologyBuilder {
    io_nodes:        HashMap<NodeId, IoAdjacency>,
    transform_nodes: HashMap<NodeId, TransformMeta>,
    edges:           HashMap<EdgeId, TopoEdge>,
}

impl TopologyBuilder {
    pub(crate) fn new() -> Self {
        Self {
            io_nodes:        HashMap::new(),
            transform_nodes: HashMap::new(),
            edges:           HashMap::new(),
        }
    }

    pub(crate) fn add_io_node(&mut self, id: NodeId) -> Result<(), TopologyError> {
        if self.io_nodes.contains_key(&id) || self.transform_nodes.contains_key(&id) {
            return Err(TopologyError::DuplicateNode(id));
        }
        self.io_nodes.insert(id, IoAdjacency::default());
        Ok(())
    }

    pub(crate) fn add_transform_node(
        &mut self,
        id:      NodeId,
        erased:  ErasedTransform,
    ) -> Result<(), TopologyError> {
        if self.io_nodes.contains_key(&id) || self.transform_nodes.contains_key(&id) {
            return Err(TopologyError::DuplicateNode(id));
        }
        let n_in  = erased.schema.inputs.len();
        let n_out = erased.schema.outputs.len();
        self.transform_nodes.insert(id, TransformMeta {
            erased,
            input_edges:  vec![vec![]; n_in],
            output_edges: vec![vec![]; n_out],
        });
        Ok(())
    }

    pub(crate) fn add_edge(
        &mut self,
        from:          Endpoint,
        to:            Endpoint,
        is_collection: bool,
    ) -> Result<EdgeId, TopologyError> {
        // Check for duplicate.
        for e in self.edges.values() {
            if e.from == from && e.to == to {
                return Err(TopologyError::DuplicateEdge { from, to });
            }
        }

        let eid = EdgeId::next();
        let edge = TopoEdge { from, to, is_collection };

        // Register on source.
        match from {
            Endpoint::Io(n) => {
                self.io_nodes.get_mut(&n)
                    .ok_or(TopologyError::NodeNotFound(n))?
                    .outgoing.push(eid);
            }
            Endpoint::TransformOutput { transform, slot } => {
                let t = self.transform_nodes.get_mut(&transform)
                    .ok_or(TopologyError::NodeNotFound(transform))?;
                if slot >= t.output_edges.len() {
                    return Err(TopologyError::SlotOutOfRange { node: transform, slot });
                }
                t.output_edges[slot].push(eid);
            }
            Endpoint::TransformInput { .. } => {
                return Err(TopologyError::TypeConflict(
                    "TransformInput cannot be an edge source".into()
                ));
            }
        }

        // Register on target.
        match to {
            Endpoint::Io(n) => {
                let node = self.io_nodes.get_mut(&n)
                    .ok_or(TopologyError::NodeNotFound(n))?;
                node.incoming = Some(eid);
            }
            Endpoint::TransformInput { transform, slot } => {
                let t = self.transform_nodes.get_mut(&transform)
                    .ok_or(TopologyError::NodeNotFound(transform))?;
                if slot >= t.input_edges.len() {
                    return Err(TopologyError::SlotOutOfRange { node: transform, slot });
                }
                t.input_edges[slot].push(eid);
            }
            Endpoint::TransformOutput { .. } => {
                return Err(TopologyError::TypeConflict(
                    "TransformOutput cannot be an edge target".into()
                ));
            }
        }

        self.edges.insert(eid, edge);
        Ok(eid)
    }

    /// Freeze into an immutable [`Topology`].
    /// Computes the topological order of all transform nodes.
    pub(crate) fn freeze(self, registry: Arc<ValueTypeRegistry>) -> Topology {
        let topo_order = compute_topo_order(&self.transform_nodes, &self.edges);
        Topology {
            io_nodes:        self.io_nodes,
            transform_nodes: self.transform_nodes,
            edges:           self.edges,
            topo_order,
            registry,
        }
    }
}

// ---------------------------------------------------------------------------
// Topology — immutable after build
// ---------------------------------------------------------------------------

/// Immutable static graph topology.  Shared freely (no lock) across all
/// concurrent tasks during an update cycle.
pub(crate) struct Topology {
    pub(crate) io_nodes:        HashMap<NodeId, IoAdjacency>,
    pub(crate) transform_nodes: HashMap<NodeId, TransformMeta>,
    pub(crate) edges:           HashMap<EdgeId, TopoEdge>,
    /// Pre-computed full topological order of all transform nodes.
    pub(crate) topo_order:      Vec<NodeId>,
    /// Shared value type registry.
    pub(crate) registry:        Arc<ValueTypeRegistry>,
}

impl Topology {
    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Topological order of all transform nodes (stable, pre-computed).
    pub(crate) fn topo_order(&self) -> &[NodeId] { &self.topo_order }

    pub(crate) fn transform_meta(&self, id: NodeId) -> Option<&TransformMeta> {
        self.transform_nodes.get(&id)
    }

    pub(crate) fn io_adjacency(&self, id: NodeId) -> Option<&IoAdjacency> {
        self.io_nodes.get(&id)
    }

    pub(crate) fn edge(&self, id: EdgeId) -> Option<&TopoEdge> {
        self.edges.get(&id)
    }

    /// Return the source endpoints feeding a given transform's input slot
    /// (traverses through intermediate IoNodes).
    pub(crate) fn input_slot_sources(&self, transform: NodeId, slot: usize) -> Vec<NodeId> {
        let meta = match self.transform_nodes.get(&transform) { Some(m) => m, None => return vec![] };
        let edge_ids = match meta.input_edges.get(slot) { Some(ids) => ids, None => return vec![] };
        let mut sources = vec![];
        for &eid in edge_ids {
            if let Some(edge) = self.edges.get(&eid) {
                match edge.from {
                    Endpoint::TransformOutput { transform: src, .. } => { sources.push(src); }
                    Endpoint::Io(io_n) => {
                        // Traverse back through the IoNode.
                        if let Some(io_adj) = self.io_nodes.get(&io_n) {
                            if let Some(in_eid) = io_adj.incoming {
                                if let Some(in_edge) = self.edges.get(&in_eid) {
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

    /// Is the edge feeding input slot `slot` of `transform` a collection edge?
    /// Checks the actual edge type (set during build), then falls back to schema.
    pub(crate) fn input_slot_is_collection(&self, transform: NodeId, slot: usize) -> bool {
        let meta = match self.transform_nodes.get(&transform) { Some(m) => m, None => return false };
        if let Some(eids) = meta.input_edges.get(slot) {
            if let Some(&eid) = eids.first() {
                if let Some(edge) = self.edges.get(&eid) {
                    return edge.is_collection;
                }
            }
        }
        meta.layout().inputs.get(slot).map(|s| s.is_col).unwrap_or(false)
    }

    /// Return all output edge (EdgeId, target Endpoint) pairs for a slot.
    pub(crate) fn output_edge_targets(&self, transform: NodeId, slot: usize)
        -> Vec<(EdgeId, Endpoint)>
    {
        let meta = match self.transform_nodes.get(&transform) { Some(m) => m, None => return vec![] };
        meta.output_edges.get(slot)
            .map(|eids| eids.iter().filter_map(|&eid| {
                self.edges.get(&eid).map(|e| (eid, e.to))
            }).collect())
            .unwrap_or_default()
    }

    pub(crate) fn all_transform_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.transform_nodes.keys().copied()
    }
}

// ---------------------------------------------------------------------------
// Topo-sort (private, called once at freeze time)
// ---------------------------------------------------------------------------

fn compute_topo_order(
    transforms: &HashMap<NodeId, TransformMeta>,
    edges:      &HashMap<EdgeId, TopoEdge>,
) -> Vec<NodeId> {
    let mut adj:       HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut in_degree: HashMap<NodeId, usize>       = HashMap::new();

    for id in transforms.keys() { in_degree.entry(*id).or_insert(0); }

    // Build reverse index: IoNode → downstream TransformInput endpoints.
    let mut io_out: HashMap<NodeId, Vec<Endpoint>> = HashMap::new();
    for edge in edges.values() {
        if let Endpoint::Io(n) = edge.from {
            io_out.entry(n).or_default().push(edge.to);
        }
    }

    let mut seen: std::collections::HashSet<(NodeId, NodeId)> = std::collections::HashSet::new();

    for edge in edges.values() {
        let src_t = match edge.from {
            Endpoint::TransformOutput { transform, .. } => Some(transform),
            _ => None,
        };
        if let Some(s) = src_t {
            // Direct: TransformOutput → TransformInput.
            if let Endpoint::TransformInput { transform: d, .. } = edge.to {
                if seen.insert((s, d)) {
                    adj.entry(s).or_default().push(d);
                    *in_degree.entry(d).or_insert(0) += 1;
                }
            }
            // Indirect: TransformOutput → IoNode → TransformInput.
            if let Endpoint::Io(io_n) = edge.to {
                if let Some(downstreams) = io_out.get(&io_n) {
                    for &ds in downstreams {
                        if let Endpoint::TransformInput { transform: d, .. } = ds {
                            if seen.insert((s, d)) {
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
    let mut order = Vec::with_capacity(transforms.len());
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
    order
}
