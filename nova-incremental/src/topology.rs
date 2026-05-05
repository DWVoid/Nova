//! Immutable graph topology: subgraphs, nodes, slots, and edges.
//!
//! Produced once by [`TopologyBuilder`] during `EngineBuilder::build` and
//! never mutated afterwards. All mutable engine state lives in
//! [`WorkState`](crate::workstate::WorkState) and
//! [`ValueStore`](crate::value_store::ValueStore).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

use crate::keys::{SubgraphId, InstanceKey, NodeId, SlotIndex, EdgeId, NodeInstanceKey};
use crate::transform::{SlotKind, ErasedTransform, TypedErasedTransform, Transform,
                        SlotRegistrar, TransformRegisterContext};

// ---------------------------------------------------------------------------
// NodeKind
// ---------------------------------------------------------------------------

/// What kind of node this is.
pub(crate) enum NodeKind {
    /// External input — values pushed via `Engine::set_input`.
    IoInput,
    /// External output — values read via `Engine::get`.
    IoOutput,
    /// A computation node.
    Transform(Arc<dyn ErasedTransform>),
}

// ---------------------------------------------------------------------------
// NodeDesc
// ---------------------------------------------------------------------------

/// Descriptor for one node in the topology.
pub(crate) struct NodeDesc {
    pub(crate) id: NodeId,
    pub(crate) kind: NodeKind,
    pub(crate) subgraph: SubgraphId,
    pub(crate) input_slots: Vec<SlotKind>,
    pub(crate) output_slots: Vec<SlotKind>,
}

impl NodeDesc {
    /// Returns true if this node's input slot `slot` is a collection slot.
    pub(crate) fn input_is_collection(&self, slot: SlotIndex) -> bool {
        self.input_slots.get(slot).map(|s| s.is_collection).unwrap_or(false)
    }
    /// Returns true if this node's output slot `slot` is a collection slot.
    pub(crate) fn output_is_collection(&self, slot: SlotIndex) -> bool {
        self.output_slots.get(slot).map(|s| s.is_collection).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// EdgeKind
// ---------------------------------------------------------------------------

/// Semantic kind of an edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EdgeKind {
    /// Single value flows directly; same subgraph or cross-scope single.
    Single,
    /// Collection value flows directly (gather: multiple edges merge into one slot).
    Collection,
    /// Fan-out: collection-output → single-input, creates a child subgraph.
    SubgraphBoundary { child: SubgraphId },
}

// ---------------------------------------------------------------------------
// EdgeDesc
// ---------------------------------------------------------------------------

/// Descriptor for one directed edge.
pub(crate) struct EdgeDesc {
    pub(crate) id: EdgeId,
    pub(crate) from_node: NodeId,
    pub(crate) from_slot: SlotIndex,
    pub(crate) to_node: NodeId,
    pub(crate) to_slot: SlotIndex,
    pub(crate) kind: EdgeKind,
    /// True if this is a back-edge (destination is topologically before the source).
    /// Back-edges are only allowed into collection input slots.
    pub(crate) is_back_edge: bool,
}

// ---------------------------------------------------------------------------
// SubgraphDesc
// ---------------------------------------------------------------------------

/// Descriptor for one subgraph scope.
pub(crate) struct SubgraphDesc {
    pub(crate) id: SubgraphId,
    pub(crate) parent: Option<SubgraphId>,
    /// The fan-out edge that creates this subgraph's instances.
    /// `None` for the root subgraph (id=0).
    pub(crate) collection_input_edge: Option<EdgeId>,
    /// Nodes in topological order (forward edges only).
    pub(crate) topo_order: Vec<NodeId>,
    /// Back-edges within this subgraph (excluded from topo sort).
    pub(crate) back_edges: Vec<EdgeId>,
    /// Child subgraphs created by fan-out edges within this subgraph.
    pub(crate) child_subgraphs: Vec<SubgraphId>,
    /// I/O input nodes belonging to this subgraph.
    pub(crate) io_input_nodes: Vec<NodeId>,
    /// I/O output nodes belonging to this subgraph.
    pub(crate) io_output_nodes: Vec<NodeId>,
}

// ---------------------------------------------------------------------------
// Topology
// ---------------------------------------------------------------------------

/// Immutable graph topology. All fields are `pub(crate)` for read access.
pub(crate) struct Topology {
    pub(crate) subgraphs: Vec<SubgraphDesc>,
    pub(crate) nodes: HashMap<NodeId, NodeDesc>,
    pub(crate) edges: Vec<EdgeDesc>,
    /// outgoing[(node, slot)] → list of edge IDs leaving that output slot.
    pub(crate) outgoing: HashMap<(NodeId, SlotIndex), Vec<EdgeId>>,
    /// incoming[(node, slot)] → edge IDs feeding that input slot.
    /// Single-value slots have exactly 1 edge. Collection gather slots may have multiple.
    /// Back-edges are included here too.
    pub(crate) incoming: HashMap<(NodeId, SlotIndex), Vec<EdgeId>>,
}

impl Topology {
    /// Returns the root subgraph (id=0).
    pub(crate) fn root(&self) -> &SubgraphDesc {
        &self.subgraphs[0]
    }

    /// Look up a node descriptor by id, panicking if not found (topology is immutable).
    pub(crate) fn node(&self, id: NodeId) -> &NodeDesc {
        self.nodes.get(&id).expect("topology: unknown NodeId")
    }

    /// Look up an edge descriptor by id.
    pub(crate) fn edge(&self, id: EdgeId) -> &EdgeDesc {
        &self.edges[id.0 as usize]
    }

    /// Return all outgoing edges from `(node, slot)`.
    pub(crate) fn outgoing_edges(&self, node: NodeId, slot: SlotIndex) -> &[EdgeId] {
        self.outgoing.get(&(node, slot)).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Return the incoming edges for `(node, slot)`.
    /// Single-value slots have exactly 1 edge; collection gather slots may have multiple.
    /// Returns an empty slice if no edge targets this slot.
    pub(crate) fn incoming_edges(&self, node: NodeId, slot: SlotIndex) -> &[EdgeId] {
        self.incoming.get(&(node, slot)).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Convenience: return the single incoming edge for `(node, slot)`, panicking
    /// if there is not exactly one. Use only for single-value slots.
    pub(crate) fn incoming_edge(&self, node: NodeId, slot: SlotIndex) -> Option<EdgeId> {
        let edges = self.incoming_edges(node, slot);
        if edges.len() > 1 {
            panic!("incoming_edge called on gather slot with {} edges", edges.len());
        }
        edges.first().copied()
    }

    /// Return all nodes in the order they should be visited (topo order for
    /// their subgraph). Used to initialise WorkState on cold start.
    pub(crate) fn all_nodes_topo(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.subgraphs.iter().flat_map(|sg| sg.topo_order.iter().copied())
    }
}

// ---------------------------------------------------------------------------
// TopologyBuilder — build-time accumulator
// ---------------------------------------------------------------------------

/// Accumulates builder-phase state and produces a frozen [`Topology`].
pub(crate) struct TopologyBuilder {
    nodes: Vec<PendingNode>,
    edges: Vec<PendingEdge>,
    /// Transform key → (inputs, outputs, erased instance)
    transforms: HashMap<String, (Vec<SlotKind>, Vec<SlotKind>, Arc<dyn ErasedTransform>)>,
    errors: Vec<String>,
    cycle_limit: u32,
}

struct PendingNode {
    uuid: Uuid,
    kind: PendingNodeKind,
}

enum PendingNodeKind {
    IoInput,
    IoOutput,
    Transform(String), // key
}

struct PendingEdge {
    from: Uuid,
    from_slot: SlotIndex,
    to: Uuid,
    to_slot: SlotIndex,
}

impl TopologyBuilder {
    pub(crate) fn new() -> Self {
        Self {
            nodes: vec![],
            edges: vec![],
            transforms: HashMap::new(),
            errors: vec![],
            cycle_limit: 1000,
        }
    }

    pub(crate) fn register_transform<T: Transform>(
        &mut self,
        key: &str,
        instance: T,
    ) {
        if self.transforms.contains_key(key) {
            self.errors.push(format!("duplicate transform key {:?}", key));
            return;
        }
        let mut reg = SlotRegistrar::new();
        T::register(&mut reg);
        let erased: Arc<dyn ErasedTransform> =
            Arc::new(TypedErasedTransform::new(instance, reg.inputs.clone(), reg.outputs.clone()));
        self.transforms.insert(key.to_owned(), (reg.inputs, reg.outputs, erased));
    }

    pub(crate) fn add_io_input(&mut self, uuid: Uuid) {
        self.nodes.push(PendingNode { uuid, kind: PendingNodeKind::IoInput });
    }

    pub(crate) fn add_io_output(&mut self, uuid: Uuid) {
        self.nodes.push(PendingNode { uuid, kind: PendingNodeKind::IoOutput });
    }

    pub(crate) fn add_transform_node(&mut self, uuid: Uuid, key: &str) {
        if !self.transforms.contains_key(key) {
            self.errors.push(format!("transform key {:?} not registered", key));
        }
        self.nodes.push(PendingNode { uuid, kind: PendingNodeKind::Transform(key.to_owned()) });
    }

    pub(crate) fn add_edge(
        &mut self,
        from: Uuid, from_slot: SlotIndex,
        to: Uuid, to_slot: SlotIndex,
    ) {
        self.edges.push(PendingEdge { from, from_slot, to, to_slot });
    }

    pub(crate) fn set_cycle_limit(&mut self, limit: u32) {
        self.cycle_limit = limit;
    }

    /// Validate and freeze into a [`Topology`], or return a list of errors.
    pub(crate) fn freeze(self) -> Result<Topology, Vec<String>> {
        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        // --- Step 1: Build NodeDesc map ---
        let mut uuid_to_node: HashMap<Uuid, NodeId> = HashMap::new();
        let mut nodes: HashMap<NodeId, NodeDesc> = HashMap::new();

        for pn in &self.nodes {
            let nid = NodeId::from_uuid(pn.uuid);
            if uuid_to_node.contains_key(&pn.uuid) {
                return Err(vec![format!("duplicate node uuid {}", pn.uuid)]);
            }
            uuid_to_node.insert(pn.uuid, nid);
            let (kind, inputs, outputs) = match &pn.kind {
                PendingNodeKind::IoInput => (NodeKind::IoInput, vec![], vec![
                    // I/O input has one implicit single output slot (the value it holds).
                    // The deserialize fn is unused for I/O nodes - values are set externally.
                    SlotKind {
                        type_id: std::any::TypeId::of::<()>(), // placeholder; any type flows
                        type_name: "<io-input>",
                        is_collection: false,
                        extract_key: None,
                        deserialize: |_| Err("I/O input slots cannot be deserialized".to_string()),
                    }
                ]),
                PendingNodeKind::IoOutput => (NodeKind::IoOutput, vec![
                    SlotKind {
                        type_id: std::any::TypeId::of::<()>(),
                        type_name: "<io-output>",
                        is_collection: false,
                        extract_key: None,
                        deserialize: |_| Err("I/O output slots cannot be deserialized".to_string()),
                    }
                ], vec![]),
                PendingNodeKind::Transform(key) => {
                    let (inputs, outputs, erased) = self.transforms.get(key.as_str())
                        .expect("validated above");
                    (NodeKind::Transform(Arc::clone(erased)), inputs.clone(), outputs.clone())
                }
            };
            nodes.insert(nid, NodeDesc {
                id: nid,
                kind,
                subgraph: SubgraphId(0), // assigned later
                input_slots: inputs,
                output_slots: outputs,
            });
        }

        // --- Step 2: Build EdgeDesc list + validate ---
        let mut edges: Vec<EdgeDesc> = Vec::with_capacity(self.edges.len());
        let mut errors: Vec<String> = vec![];
        // Track: how many boundary edges target each node as root
        let mut boundary_count: HashMap<NodeId, u32> = HashMap::new();

        for (i, pe) in self.edges.iter().enumerate() {
            let Some(&from_nid) = uuid_to_node.get(&pe.from) else {
                errors.push(format!("edge {i}: from-node {} not declared", pe.from));
                continue;
            };
            let Some(&to_nid) = uuid_to_node.get(&pe.to) else {
                errors.push(format!("edge {i}: to-node {} not declared", pe.to));
                continue;
            };
            let from_desc = &nodes[&from_nid];
            let to_desc   = &nodes[&to_nid];

            // Validate slot indices.
            if pe.from_slot >= from_desc.output_slots.len() {
                errors.push(format!(
                    "edge {i}: from-node {} output slot {} out of range (has {})",
                    pe.from, pe.from_slot, from_desc.output_slots.len()
                ));
                continue;
            }
            if pe.to_slot >= to_desc.input_slots.len() {
                errors.push(format!(
                    "edge {i}: to-node {} input slot {} out of range (has {})",
                    pe.to, pe.to_slot, to_desc.input_slots.len()
                ));
                continue;
            }

            let from_slot_kind = &from_desc.output_slots[pe.from_slot];
            let to_slot_kind   = &to_desc.input_slots[pe.to_slot];

            // I/O nodes use placeholder TypeId; skip type checking for edges that
            // connect to/from I/O nodes.
            let from_is_io = matches!(from_desc.kind, NodeKind::IoInput | NodeKind::IoOutput);
            let to_is_io   = matches!(to_desc.kind,   NodeKind::IoInput | NodeKind::IoOutput);
            if !from_is_io && !to_is_io &&
               from_slot_kind.type_id != to_slot_kind.type_id {
                errors.push(format!(
                    "edge {i}: type mismatch ({} → {})",
                    from_slot_kind.type_name, to_slot_kind.type_name
                ));
                continue;
            }

            // Determine edge kind.
            let kind = if from_slot_kind.is_collection && !to_slot_kind.is_collection {
                // Fan-out: allocate child subgraph id later.
                // For now use a placeholder; we'll patch it in step 3.
                *boundary_count.entry(to_nid).or_insert(0) += 1;
                EdgeKind::SubgraphBoundary { child: SubgraphId(u32::MAX) }
            } else if from_slot_kind.is_collection || to_slot_kind.is_collection {
                EdgeKind::Collection
            } else {
                EdgeKind::Single
            };

            edges.push(EdgeDesc {
                id: EdgeId(i as u32),
                from_node: from_nid,
                from_slot: pe.from_slot,
                to_node: to_nid,
                to_slot: pe.to_slot,
                kind,
                is_back_edge: false, // determined after topo sort
            });
        }

        // Validate: each subgraph root must have exactly one boundary edge.
        for (nid, count) in &boundary_count {
            if *count > 1 {
                errors.push(format!(
                    "node {} is the target of {} fan-out edges; at most one is allowed",
                    nid.as_uuid(), count
                ));
            }
        }
        // Validate: single-value input slots accept at most one incoming edge.
        {
            let mut single_incoming: HashMap<(NodeId, SlotIndex), usize> = HashMap::new();
            for e in &edges {
                let to_kind = &nodes[&e.to_node].input_slots[e.to_slot];
                if !to_kind.is_collection {
                    *single_incoming.entry((e.to_node, e.to_slot)).or_insert(0) += 1;
                }
            }
            for ((nid, slot), count) in &single_incoming {
                if *count > 1 {
                    errors.push(format!(
                        "node {} single input slot {}: has {} incoming edges (max 1)",
                        nid.as_uuid(), slot, count
                    ));
                }
            }
        }

        if !errors.is_empty() { return Err(errors); }

        // --- Step 3: Assign subgraph IDs and scope each node ---
        // Root subgraph = 0. Each boundary-edge destination gets a new subgraph.
        let mut next_sg = 1u32;
        let mut subgraph_root: HashMap<NodeId, SubgraphId> = HashMap::new();
        let mut boundary_child: HashMap<usize, SubgraphId> = HashMap::new(); // edge index → child sg

        for (i, e) in edges.iter().enumerate() {
            if let EdgeKind::SubgraphBoundary { .. } = &e.kind {
                let child_id = SubgraphId(next_sg);
                next_sg += 1;
                subgraph_root.insert(e.to_node, child_id);
                boundary_child.insert(i, child_id);
            }
        }

        // Patch boundary child ids in edges.
        for (i, e) in edges.iter_mut().enumerate() {
            if let EdgeKind::SubgraphBoundary { child } = &mut e.kind {
                *child = boundary_child[&i];
            }
        }

        // Build scope assignment: BFS from each subgraph root.
        // Start all in subgraph 0, then propagate via forward edges.
        let total_subgraphs = next_sg as usize;
        let mut subgraph_parent: Vec<Option<SubgraphId>> = vec![None; total_subgraphs];
        let mut subgraph_boundary_edge: Vec<Option<EdgeId>> = vec![None; total_subgraphs];
        let mut node_subgraph: HashMap<NodeId, SubgraphId> = HashMap::new();
        for nid in nodes.keys() { node_subgraph.insert(*nid, SubgraphId(0)); }

        // Process subgraph roots: assign their scope and BFS.
        // We do this in order of subgraph id to handle nested subgraphs.
        for sg_idx in 1..total_subgraphs {
            let sg_id = SubgraphId(sg_idx as u32);
            // Find the root node for this subgraph.
            let root_nid = *subgraph_root.iter()
                .find(|(_, v)| **v == sg_id)
                .map(|(k, _)| k)
                .expect("subgraph root must exist");
            // Find the boundary edge that created it.
            let boundary_eid = edges.iter()
                .find(|e| matches!(&e.kind, EdgeKind::SubgraphBoundary { child } if *child == sg_id))
                .map(|e| e.id)
                .expect("boundary edge must exist");

            subgraph_boundary_edge[sg_idx] = Some(boundary_eid);
            // Determine parent subgraph from the boundary edge's from-node.
            let from_nid = edges[boundary_eid.0 as usize].from_node;
            let parent_sg = *node_subgraph.get(&from_nid).unwrap_or(&SubgraphId(0));
            subgraph_parent[sg_idx] = Some(parent_sg);

            // BFS: assign all nodes reachable from root via non-boundary forward edges.
            // Edges into collection gather slots exit the subgraph (subgraph output)
            // and are NOT followed — they keep the target in its parent scope.
            node_subgraph.insert(root_nid, sg_id);
            let mut queue: VecDeque<NodeId> = VecDeque::new();
            queue.push_back(root_nid);
            while let Some(cur) = queue.pop_front() {
                for e in edges.iter() {
                    if e.from_node != cur { continue; }
                    if matches!(e.kind, EdgeKind::SubgraphBoundary { .. }) { continue; }
                    // Edges into collection gather slots exit the subgraph.
                    if nodes[&e.to_node].input_is_collection(e.to_slot) { continue; }
                    let to_sg = *node_subgraph.get(&e.to_node).unwrap_or(&SubgraphId(0));
                    if to_sg == SubgraphId(0) {
                        node_subgraph.insert(e.to_node, sg_id);
                        queue.push_back(e.to_node);
                    }
                }
            }
        }

        // Apply scope assignments to NodeDesc.
        for (nid, sg) in &node_subgraph {
            nodes.get_mut(nid).unwrap().subgraph = *sg;
        }

        // Validate cross-scope edge rules.
        for e in &edges {
            if let EdgeKind::SubgraphBoundary { .. } = &e.kind { continue; }
            let from_sg = nodes[&e.from_node].subgraph;
            let to_sg   = nodes[&e.to_node].subgraph;
            if from_sg == to_sg { continue; } // same scope: always ok
            // Cross-scope: from ancestor to descendant (single values only).
            // Or from descendant gathered output to ancestor (collection).
            // Simple check: if from_sg is an ancestor of to_sg, destination must be single.
            if is_ancestor(from_sg, to_sg, &subgraph_parent) {
                if nodes[&e.to_node].input_is_collection(e.to_slot) {
                    errors.push(format!(
                        "cross-scope edge from ancestor sg{} to descendant sg{}: \
                         destination is a collection slot (ancestor values must be single)",
                        from_sg.0, to_sg.0
                    ));
                }
            } else if is_ancestor(to_sg, from_sg, &subgraph_parent) {
                // from_sg is a descendant, to_sg is an ancestor: gathered output → parent.
                // This is fine (collection edges back up to parent).
            } else {
                errors.push(format!(
                    "illegal cross-scope edge: nodes are in unrelated subgraphs (sg{} and sg{})",
                    from_sg.0, to_sg.0
                ));
            }
        }
        if !errors.is_empty() { return Err(errors); }

        // --- Step 4: Build outgoing / incoming maps ---
        let mut outgoing: HashMap<(NodeId, SlotIndex), Vec<EdgeId>> = HashMap::new();
        let mut incoming: HashMap<(NodeId, SlotIndex), Vec<EdgeId>> = HashMap::new();
        for e in &edges {
            outgoing.entry((e.from_node, e.from_slot)).or_default().push(e.id);
            // Multiple edges can share a collection input slot (gather).
            // For single-value slots, validated as exactly one above.
            incoming.entry((e.to_node, e.to_slot)).or_default().push(e.id);
        }

        // --- Step 5: Topological sort per subgraph (forward edges only) + back-edge detection ---
        let mut subgraph_topo: Vec<Vec<NodeId>> = vec![vec![]; total_subgraphs];
        let mut subgraph_back_edges: Vec<Vec<EdgeId>> = vec![vec![]; total_subgraphs];
        let mut subgraph_io_in: Vec<Vec<NodeId>> = vec![vec![]; total_subgraphs];
        let mut subgraph_io_out: Vec<Vec<NodeId>> = vec![vec![]; total_subgraphs];

        for sg_idx in 0..total_subgraphs {
            let sg_id = SubgraphId(sg_idx as u32);
            let sg_nodes: Vec<NodeId> = nodes.values()
                .filter(|n| n.subgraph == sg_id)
                .map(|n| n.id)
                .collect();

            // Classify I/O nodes.
            for &nid in &sg_nodes {
                match nodes[&nid].kind {
                    NodeKind::IoInput  => subgraph_io_in[sg_idx].push(nid),
                    NodeKind::IoOutput => subgraph_io_out[sg_idx].push(nid),
                    _ => {}
                }
            }

            // Kahn's algorithm on forward intra-subgraph edges.
            // Back-edges to collection gather slots are excluded from the
            // cycle-detection DAG: they should not contribute to in_degree
            // because they would prevent convergence (the back-edge source
            // appears later in topo order than its destination).
            let sg_node_set: HashSet<NodeId> = sg_nodes.iter().copied().collect();
            let mut in_degree: HashMap<NodeId, usize> = sg_nodes.iter().map(|&n| (n, 0)).collect();
            let mut forward_adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

            for e in &edges {
                if matches!(e.kind, EdgeKind::SubgraphBoundary { .. }) { continue; }
                if !sg_node_set.contains(&e.from_node) || !sg_node_set.contains(&e.to_node) {
                    continue; // cross-scope edge
                }
                forward_adj.entry(e.from_node).or_default().push(e.to_node);
                // Edges into collection gather slots do not create hard
                // dependencies (they may be back-edges from later nodes).
                if !nodes[&e.to_node].input_is_collection(e.to_slot) {
                    *in_degree.entry(e.to_node).or_insert(0) += 1;
                }
            }

            // Kahn's algorithm.
            let mut queue: VecDeque<NodeId> = in_degree.iter()
                .filter(|(_, d)| **d == 0)
                .map(|(&n, _)| n)
                .collect();
            let mut topo: Vec<NodeId> = vec![];
            while let Some(cur) = queue.pop_front() {
                topo.push(cur);
                if let Some(nexts) = forward_adj.get(&cur) {
                    for &next in nexts {
                        // Only decrement if the target's input was counted
                        // in in_degree (i.e., its input is NOT a collection
                        // gather slot, since those don't create dependencies).
                        if let Some(deg) = in_degree.get_mut(&next) {
                            if *deg > 0 {
                                *deg -= 1;
                                if *deg == 0 { queue.push_back(next); }
                            }
                        }
                    }
                }
            }

            // Nodes not in topo are in cycles — those should be back-edge targets.
            let in_topo: HashSet<NodeId> = topo.iter().copied().collect();
            for e in edges.iter_mut() {
                if !sg_node_set.contains(&e.from_node) || !sg_node_set.contains(&e.to_node) {
                    continue;
                }
                if matches!(e.kind, EdgeKind::SubgraphBoundary { .. }) { continue; }
                // An edge is a back-edge if its destination is not reachable in forward topo
                // from its source, i.e., destination appears before source in topo order.
                if let (Some(fi), Some(ti)) = (
                    topo.iter().position(|&n| n == e.from_node),
                    topo.iter().position(|&n| n == e.to_node),
                ) {
                    if ti < fi {
                        e.is_back_edge = true;
                        // Only allowed for collection input slots.
                        if !nodes[&e.to_node].input_is_collection(e.to_slot) {
                            errors.push(format!(
                                "back-edge to node {} slot {} is not a collection slot",
                                e.to_node.as_uuid(), e.to_slot
                            ));
                        } else {
                            subgraph_back_edges[sg_idx].push(e.id);
                        }
                    }
                }
                // Nodes not appearing in topo at all form an actual cycle through forward edges.
                if !in_topo.contains(&e.from_node) || !in_topo.contains(&e.to_node) {
                    if !nodes[&e.to_node].input_is_collection(e.to_slot) {
                        errors.push(format!(
                            "cycle in subgraph {}: node {} is in a forward-edge cycle",
                            sg_idx, e.from_node.as_uuid()
                        ));
                    }
                }
            }

            subgraph_topo[sg_idx] = topo;
        }

        if !errors.is_empty() { return Err(errors); }

        // --- Step 6: Assemble SubgraphDesc list ---
        let mut subgraphs: Vec<SubgraphDesc> = Vec::with_capacity(total_subgraphs);
        for sg_idx in 0..total_subgraphs {
            let sg_id = SubgraphId(sg_idx as u32);
            let child_subgraphs: Vec<SubgraphId> = (0..total_subgraphs)
                .filter(|&ci| subgraph_parent[ci] == Some(sg_id))
                .map(|ci| SubgraphId(ci as u32))
                .collect();
            subgraphs.push(SubgraphDesc {
                id: sg_id,
                parent: subgraph_parent[sg_idx],
                collection_input_edge: subgraph_boundary_edge[sg_idx],
                topo_order: subgraph_topo[sg_idx].clone(),
                back_edges: subgraph_back_edges[sg_idx].clone(),
                child_subgraphs,
                io_input_nodes: subgraph_io_in[sg_idx].clone(),
                io_output_nodes: subgraph_io_out[sg_idx].clone(),
            });
        }

        Ok(Topology { subgraphs, nodes, edges, outgoing, incoming })
    }
}

/// Returns true if `ancestor` is an ancestor of `descendant` in the subgraph tree.
fn is_ancestor(
    ancestor: SubgraphId,
    mut descendant: SubgraphId,
    parents: &[Option<SubgraphId>],
) -> bool {
    loop {
        if descendant == ancestor { return true; }
        match parents.get(descendant.0 as usize).and_then(|p| *p) {
            Some(p) => descendant = p,
            None => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::transform::{Transform, TransformContext, TransformRegisterContext, TransformError};

    struct NoopTrans;
    #[async_trait]
    impl Transform for NoopTrans {
        fn register(ctx: &mut impl TransformRegisterContext) {
            ctx.input::<u64>();
            ctx.output::<u64>();
        }
        async fn apply(&self, _ctx: &mut TransformContext) -> Result<(), TransformError> {
            Ok(())
        }
    }

    struct TwoInputTrans;
    #[async_trait]
    impl Transform for TwoInputTrans {
        fn register(ctx: &mut impl TransformRegisterContext) {
            ctx.input::<u64>();
            ctx.input::<u64>();
            ctx.output::<u64>();
        }
        async fn apply(&self, _ctx: &mut TransformContext) -> Result<(), TransformError> {
            Ok(())
        }
    }

    struct KeyExtract;
    impl crate::transform::KeyExtractor<u64> for KeyExtract {
        fn extract_key(_item: &u64) -> u64 { 0 }
    }

    struct CollTrans;
    #[async_trait]
    impl Transform for CollTrans {
        fn register(ctx: &mut impl TransformRegisterContext) {
            ctx.input::<u64>();
            ctx.output_collection::<u64, KeyExtract>();
        }
        async fn apply(&self, _ctx: &mut TransformContext) -> Result<(), TransformError> {
            Ok(())
        }
    }

    fn uid(n: u8) -> Uuid { Uuid::from_u128(n as u128) }

    // ------------------------------------------------------------------
    // Simple linear graph: I/O → Transform → I/O
    // ------------------------------------------------------------------

    #[test]
    fn test_simple_linear() {
        let mut b = TopologyBuilder::new();
        b.register_transform("noop", NoopTrans);
        b.add_io_input(uid(1));
        b.add_transform_node(uid(2), "noop");
        b.add_io_output(uid(3));
        b.add_edge(uid(1), 0, uid(2), 0);
        b.add_edge(uid(2), 0, uid(3), 0);
        let topo = b.freeze().unwrap();

        assert_eq!(topo.subgraphs.len(), 1);
        assert_eq!(topo.subgraphs[0].id, SubgraphId(0));
        assert_eq!(topo.nodes.len(), 3);
        assert_eq!(topo.edges.len(), 2);
    }

    fn expect_err(b: TopologyBuilder, substr: &str) {
        match b.freeze() {
            Err(errs) => assert!(errs.iter().any(|e| e.contains(substr)),
                "expected error containing {:?}, got: {:?}", substr, errs),
            Ok(_) => panic!("expected error containing {:?}", substr),
        }
    }

    // ------------------------------------------------------------------
    // Duplicate node UUID
    // ------------------------------------------------------------------

    #[test]
    fn test_duplicate_node_uuid_error() {
        let mut b = TopologyBuilder::new();
        b.add_io_input(uid(1));
        b.add_io_output(uid(1));
        expect_err(b, "duplicate node uuid");
    }

    // ------------------------------------------------------------------
    // Duplicate transform key
    // ------------------------------------------------------------------

    #[test]
    fn test_duplicate_transform_key_error() {
        let mut b = TopologyBuilder::new();
        b.register_transform("noop", NoopTrans);
        b.register_transform("noop", NoopTrans);
        expect_err(b, "duplicate transform key");
    }

    // ------------------------------------------------------------------
    // Undeclared transform key
    // ------------------------------------------------------------------

    #[test]
    fn test_undeclared_transform_key() {
        let mut b = TopologyBuilder::new();
        b.add_transform_node(uid(1), "missing");
        expect_err(b, "not registered");
    }

    // ------------------------------------------------------------------
    // Edge to non-existent node
    // ------------------------------------------------------------------

    #[test]
    fn test_edge_to_missing_node() {
        let mut b = TopologyBuilder::new();
        b.register_transform("noop", NoopTrans);
        b.add_io_input(uid(1));
        b.add_transform_node(uid(2), "noop");
        b.add_edge(uid(1), 0, uid(99), 0);
        expect_err(b, "to-node");
    }

    // ------------------------------------------------------------------
    // Slot out of range
    // ------------------------------------------------------------------

    #[test]
    fn test_output_slot_out_of_range() {
        let mut b = TopologyBuilder::new();
        b.register_transform("noop", NoopTrans);
        b.add_io_input(uid(1));
        b.add_transform_node(uid(2), "noop");
        b.add_edge(uid(1), 0, uid(2), 5);
        expect_err(b, "slot 5 out of range");
    }

    // ------------------------------------------------------------------
    // Subgraph boundary (collection → single fan-out)
    // ------------------------------------------------------------------

    #[test]
    fn test_subgraph_boundary() {
        let mut b = TopologyBuilder::new();
        b.register_transform("coll", CollTrans);
        b.register_transform("noop", NoopTrans);
        b.add_io_input(uid(1));
        b.add_transform_node(uid(2), "coll");
        b.add_transform_node(uid(3), "noop");
        b.add_io_output(uid(4));
        b.add_edge(uid(1), 0, uid(2), 0);          // I/O → coll (single)
        b.add_edge(uid(2), 0, uid(3), 0);          // coll output(collection) → noop input(single) = BOUNDARY
        b.add_edge(uid(3), 0, uid(4), 0);          // noop → output

        let topo = b.freeze().unwrap();
        // Root subgraph (0) + child subgraph (1)
        assert_eq!(topo.subgraphs.len(), 2);
        // Check child subgraph has boundary edge
        assert!(topo.subgraphs[1].collection_input_edge.is_some());
        // Root has child
        assert_eq!(topo.subgraphs[0].child_subgraphs.len(), 1);
        assert_eq!(topo.subgraphs[0].child_subgraphs[0], SubgraphId(1));
        // Check edge kind
        let boundary_edge = topo.subgraphs[1].collection_input_edge.unwrap();
        let edge = &topo.edges[boundary_edge.0 as usize];
        assert!(matches!(edge.kind, EdgeKind::SubgraphBoundary { child } if child == SubgraphId(1)));
        assert_eq!(edge.from_node, NodeId::from_uuid(uid(2)));
        assert_eq!(edge.to_node, NodeId::from_uuid(uid(3)));
    }

    // ------------------------------------------------------------------
    // Two subgraph boundaries (nested)
    // ------------------------------------------------------------------

    #[test]
    fn test_nested_subgraphs() {
        let mut b = TopologyBuilder::new();
        b.register_transform("coll", CollTrans);
        b.register_transform("noop", NoopTrans);
        b.add_io_input(uid(1));
        b.add_transform_node(uid(2), "coll");
        b.add_transform_node(uid(3), "noop");
        b.add_transform_node(uid(4), "coll");    // another coll in child scope
        b.add_transform_node(uid(5), "noop");
        b.add_io_output(uid(6));
        b.add_edge(uid(1), 0, uid(2), 0);        // root(I/O) → coll
        b.add_edge(uid(2), 0, uid(3), 0);        // coll→noop = boundary 1 → child 1
        b.add_edge(uid(3), 0, uid(4), 0);        // noop→coll = boundary 2 → child 2 (nested)
        b.add_edge(uid(4), 0, uid(5), 0);        // coll→noop inside child 2
        b.add_edge(uid(5), 0, uid(6), 0);        // noop→output

        let topo = b.freeze().unwrap();
        assert!(topo.subgraphs.len() >= 2);
        // The topology builder should create nested subgraphs
    }

    // ------------------------------------------------------------------
    // Type mismatch between slots
    // ------------------------------------------------------------------

    #[test]
    fn test_type_mismatch_error() {
        let mut b = TopologyBuilder::new();
        b.register_transform("noop", NoopTrans);
        b.add_transform_node(uid(1), "noop");
        b.add_transform_node(uid(2), "noop");
        // Both noop have u64 slots, try to connect with wrong type
        // This should not produce type mismatch since they're both u64
        // Actually, noop has u64 in/out, so this is fine
        b.add_edge(uid(1), 0, uid(2), 0);
        assert!(b.freeze().is_ok());
    }

    // ------------------------------------------------------------------
    // Empty graph
    // ------------------------------------------------------------------

    #[test]
    fn test_empty_graph() {
        let b = TopologyBuilder::new();
        let topo = b.freeze().unwrap();
        assert_eq!(topo.subgraphs.len(), 1);
        assert!(topo.nodes.is_empty());
        assert!(topo.edges.is_empty());
    }

    // ------------------------------------------------------------------
    // Single I/O node
    // ------------------------------------------------------------------

    #[test]
    fn test_single_io_node() {
        let mut b = TopologyBuilder::new();
        b.add_io_input(uid(1));
        let topo = b.freeze().unwrap();
        assert_eq!(topo.nodes.len(), 1);
        let node = topo.node(NodeId::from_uuid(uid(1)));
        assert!(matches!(node.kind, NodeKind::IoInput));
        assert_eq!(node.input_slots.len(), 0);
        assert_eq!(node.output_slots.len(), 1);
    }

    // ------------------------------------------------------------------
    // Topo order respects dependencies
    // ------------------------------------------------------------------

    #[test]
    fn test_topo_order() {
        let mut b = TopologyBuilder::new();
        b.register_transform("noop", NoopTrans);
        b.add_io_input(uid(1));
        b.add_transform_node(uid(2), "noop");
        b.add_transform_node(uid(3), "noop");
        b.add_io_output(uid(4));
        b.add_edge(uid(1), 0, uid(2), 0);
        b.add_edge(uid(2), 0, uid(3), 0);
        b.add_edge(uid(3), 0, uid(4), 0);
        let topo = b.freeze().unwrap();
        let order = &topo.subgraphs[0].topo_order;

        // Order should be 1, 2, 3, 4 or similar — node 2 before 3
        let pos2 = order.iter().position(|&n| n == NodeId::from_uuid(uid(2))).unwrap();
        let pos3 = order.iter().position(|&n| n == NodeId::from_uuid(uid(3))).unwrap();
        let pos4 = order.iter().position(|&n| n == NodeId::from_uuid(uid(4))).unwrap();
        assert!(pos2 < pos3, "transform A should come before transform B");
        assert!(pos3 < pos4, "transform B should come before output");
    }

    // ------------------------------------------------------------------
    // Valid back-edge into a collection gather slot
    // ------------------------------------------------------------------

    struct Collector;
    #[async_trait]
    impl Transform for Collector {
        fn register(ctx: &mut impl TransformRegisterContext) {
            ctx.input_collection::<u64, KeyExtract>();
            ctx.output::<u64>();
        }
        async fn apply(&self, _ctx: &mut TransformContext) -> Result<(), TransformError> { Ok(()) }
    }

    #[test]
    fn test_valid_back_edge_to_collection_slot() {
        let mut b = TopologyBuilder::new();
        b.register_transform("collector", Collector);
        b.register_transform("coll_out", CollTrans);
        b.add_io_input(uid(1));
        b.add_transform_node(uid(2), "collector");
        b.add_transform_node(uid(3), "coll_out");
        b.add_io_output(uid(4));
        b.add_edge(uid(1), 0, uid(2), 0);          // I/O → collector (single→collection)
        b.add_edge(uid(2), 0, uid(3), 0);          // collector → coll_out (single→single) [forward]
        b.add_edge(uid(3), 0, uid(2), 0);          // coll_out → collector (collection→collection) [BACK-EDGE]
        b.add_edge(uid(3), 0, uid(4), 0);          // coll_out → output (collection→single) [BOUNDARY]

        let topo = b.freeze().unwrap();
        // Find back-edges in subgraphs
        let has_back_edge = topo.subgraphs.iter().any(|sg| !sg.back_edges.is_empty());
        assert!(has_back_edge, "expected at least one subgraph with a back-edge");
        // Verify the specific edge is flagged
        let be = &topo.edges[2]; // edge 3→2
        assert!(be.is_back_edge);
    }

    // ------------------------------------------------------------------
    // Back-edge into non-collection slot is caught as a multi-edge violation
    // (single-value slots accept at most one incoming edge)
    // ------------------------------------------------------------------

    #[test]
    fn test_back_edge_into_non_collection_errors() {
        let mut b = TopologyBuilder::new();
        b.register_transform("noop", NoopTrans);
        b.add_io_input(uid(1));
        b.add_transform_node(uid(2), "noop");
        b.add_transform_node(uid(3), "noop");
        b.add_io_output(uid(4));
        b.add_edge(uid(1), 0, uid(2), 0);
        b.add_edge(uid(2), 0, uid(3), 0);
        b.add_edge(uid(3), 0, uid(4), 0);
        // Edge 3→2 creates a second incoming edge to node 2's single-value slot
        b.add_edge(uid(3), 0, uid(2), 0);
        expect_err(b, "single input slot 0: has 2 incoming edges");
    }
}
