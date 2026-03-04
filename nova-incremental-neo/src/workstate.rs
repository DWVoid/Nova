//! [`WorkState`] — per-node-instance mutable flags (present, dirty, error, hash).
//!
//! This is the only piece of engine state persisted between sessions.
//! It uses fine-grained per-instance locks to reduce contention when multiple
//! instances run in parallel.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use dashmap::DashMap;
use serde::{Serialize, Deserialize};
use tokio::sync::Mutex;

use crate::keys::{
    SubgraphId, InstanceKey, NodeId, SlotIndex, UNIT_INSTANCE,
    NodeInstanceKey, SubgraphInstanceKey, SlotStateKey,
};
use crate::topology::Topology;

// ---------------------------------------------------------------------------
// ValueHash
// ---------------------------------------------------------------------------

/// A simple 64-bit hash over the serialised value bytes, used for change detection.
pub(crate) type ValueHash = u64;

// ---------------------------------------------------------------------------
// SlotState
// ---------------------------------------------------------------------------

/// Per-output-slot flags for one node instance.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SlotState {
    pub(crate) present: bool,
    pub(crate) dirty: bool,
    pub(crate) error: bool,
    pub(crate) hash: Option<ValueHash>,
    /// For collection output slots: sorted list of known element keys.
    pub(crate) element_keys: Vec<u64>,
    /// For collection output slots: hash of each element at last evaluation.
    #[serde(skip)]
    pub(crate) element_hashes: HashMap<u64, ValueHash>,
}

impl SlotState {
    fn new_dirty() -> Self {
        Self { dirty: true, ..Default::default() }
    }
}

// ---------------------------------------------------------------------------
// NodeInstanceState
// ---------------------------------------------------------------------------

/// All mutable per-(subgraph, instance, node) state.
/// Wrapped in `Arc<Mutex<...>>` so each instance is independently lockable.
pub(crate) struct NodeInstanceState {
    /// One [`SlotState`] per output slot.
    pub(crate) output_slots: Vec<SlotState>,
    /// How many times this node has been executed in the current update session.
    pub(crate) execution_count: u32,
    /// True while a task for this node instance is currently executing.
    pub(crate) executing: bool,
}

impl NodeInstanceState {
    fn new(n_output_slots: usize) -> Self {
        Self {
            output_slots: (0..n_output_slots).map(|_| SlotState::new_dirty()).collect(),
            execution_count: 0,
            executing: false,
        }
    }

    /// Returns true if all output slots are error-free and present.
    pub(crate) fn all_outputs_present_and_ok(&self) -> bool {
        self.output_slots.iter().all(|s| s.present && !s.error)
    }

    /// Returns true if any output slot is dirty.
    pub(crate) fn any_output_dirty(&self) -> bool {
        self.output_slots.iter().any(|s| s.dirty)
    }
}

// ---------------------------------------------------------------------------
// WorkState
// ---------------------------------------------------------------------------

/// Central mutable engine state, partitioned by node instance for low contention.
///
/// `node_states` uses `DashMap` so different instances can be accessed
/// concurrently without contention at the map level. Each value is an
/// `Arc<Mutex<NodeInstanceState>>` so the per-instance lock is minimally scoped.
pub(crate) struct WorkState {
    /// Per-(SubgraphId, InstanceKey, NodeId) state, lazily created.
    node_states: DashMap<NodeInstanceKey, Arc<Mutex<NodeInstanceState>>>,
    /// Known child instance keys per (SubgraphId, parent InstanceKey).
    instances: DashMap<SubgraphInstanceKey, Arc<Mutex<Vec<InstanceKey>>>>,
    /// Tasks currently in the queue but not yet executing.
    enqueued: Mutex<HashSet<NodeInstanceKey>>,
}

impl WorkState {
    pub(crate) fn new() -> Self {
        Self {
            node_states: DashMap::new(),
            instances:   DashMap::new(),
            enqueued:    Mutex::new(HashSet::new()),
        }
    }

    // -----------------------------------------------------------------------
    // Node state access
    // -----------------------------------------------------------------------

    /// Get or create the [`NodeInstanceState`] for `key`.
    ///
    /// On first access, initialises all output slots as dirty/absent.
    pub(crate) fn get_or_create(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> Arc<Mutex<NodeInstanceState>> {
        if let Some(existing) = self.node_states.get(&key) {
            return Arc::clone(&*existing);
        }
        let n_out = topology.node(key.node).output_slots.len();
        let state = Arc::new(Mutex::new(NodeInstanceState::new(n_out)));
        self.node_states.entry(key).or_insert_with(|| Arc::clone(&state));
        // Return the canonical entry (handles race: two concurrent calls).
        Arc::clone(&*self.node_states.get(&key).unwrap())
    }

    /// Remove all node states for a given (subgraph, instance).
    pub(crate) fn remove_instance_states(
        &self,
        subgraph: SubgraphId,
        instance: InstanceKey,
        topology: &Topology,
    ) {
        let sg_desc = &topology.subgraphs[subgraph.0 as usize];
        for &node in &sg_desc.topo_order {
            let key = NodeInstanceKey::new(subgraph, instance, node);
            self.node_states.remove(&key);
        }
    }

    // -----------------------------------------------------------------------
    // Instance lifecycle
    // -----------------------------------------------------------------------

    /// Register a new child instance.
    pub(crate) async fn add_instance(
        &self,
        subgraph: SubgraphId,
        parent_instance: InstanceKey,
        element_key: InstanceKey,
    ) {
        let key = SubgraphInstanceKey::new(subgraph, parent_instance);
        let entry = self.instances.entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(vec![])));
        let mut list = entry.lock().await;
        if !list.contains(&element_key) {
            list.push(element_key);
            list.sort_unstable();
        }
    }

    /// Remove a child instance and tear down its node states.
    pub(crate) async fn remove_instance(
        &self,
        subgraph: SubgraphId,
        parent_instance: InstanceKey,
        element_key: InstanceKey,
        topology: &Topology,
    ) {
        let key = SubgraphInstanceKey::new(subgraph, parent_instance);
        if let Some(entry) = self.instances.get(&key) {
            let mut list = entry.lock().await;
            list.retain(|&k| k != element_key);
        }
        self.remove_instance_states(subgraph, element_key, topology);
    }

    /// Return the current list of known instance keys for a subgraph scope.
    pub(crate) async fn instance_keys(
        &self,
        subgraph: SubgraphId,
        parent_instance: InstanceKey,
    ) -> Vec<InstanceKey> {
        let key = SubgraphInstanceKey::new(subgraph, parent_instance);
        match self.instances.get(&key) {
            Some(entry) => entry.lock().await.clone(),
            None => vec![],
        }
    }

    // -----------------------------------------------------------------------
    // Readiness queries
    // -----------------------------------------------------------------------

    /// Returns true if all required inputs for `key` are present and error-free.
    ///
    /// For each input slot, resolves the source output slot via topology and
    /// checks its [`SlotState`].
    pub(crate) async fn is_ready(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> bool {
        let node_desc = topology.node(key.node);
        for (slot_idx, _) in node_desc.input_slots.iter().enumerate() {
            let Some(edge_id) = topology.incoming_edge(key.node, slot_idx) else {
                // No incoming edge for this slot — treat as absent.
                return false;
            };
            let edge = topology.edge(edge_id);
            if edge.is_back_edge { continue; } // back-edges don't block readiness

            // Determine the source instance key (same instance for intra-scope;
            // parent instance for cross-scope).
            let src_instance = source_instance_for_edge(edge, key.instance, topology);
            let src_key = NodeInstanceKey::new(
                topology.node(edge.from_node).subgraph,
                src_instance,
                edge.from_node,
            );
            let src_state = self.get_or_create(src_key, topology);
            let guard = src_state.lock().await;
            let slot_state = &guard.output_slots[edge.from_slot];
            if !slot_state.present || slot_state.error {
                return false;
            }
        }
        true
    }

    /// Returns true if `is_ready` and at least one input slot is dirty.
    pub(crate) async fn is_pending(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> bool {
        if !self.is_ready(key, topology).await { return false; }
        let node_desc = topology.node(key.node);
        for (slot_idx, _) in node_desc.input_slots.iter().enumerate() {
            let Some(edge_id) = topology.incoming_edge(key.node, slot_idx) else { continue; };
            let edge = topology.edge(edge_id);
            let src_instance = source_instance_for_edge(edge, key.instance, topology);
            let src_key = NodeInstanceKey::new(
                topology.node(edge.from_node).subgraph,
                src_instance,
                edge.from_node,
            );
            let src_state = self.get_or_create(src_key, topology);
            let guard = src_state.lock().await;
            if guard.output_slots[edge.from_slot].dirty { return true; }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Enqueue / execute protocol
    // -----------------------------------------------------------------------

    /// Try to enqueue `key`. Returns `true` if successfully added to the queue.
    /// Returns `false` if already enqueued or currently executing.
    pub(crate) async fn try_enqueue(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> bool {
        let mut enqueued = self.enqueued.lock().await;
        if enqueued.contains(&key) { return false; }
        // Check executing flag.
        if let Some(state_arc) = self.node_states.get(&key) {
            if state_arc.lock().await.executing { return false; }
        }
        enqueued.insert(key);
        true
    }

    /// Called at the start of task execution. Sets `executing=true`, removes from queue.
    /// Returns `false` if the state was inconsistent (stale task — abort).
    pub(crate) async fn begin_execute(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> bool {
        {
            let mut enqueued = self.enqueued.lock().await;
            enqueued.remove(&key);
        }
        let state_arc = self.get_or_create(key, topology);
        let mut guard = state_arc.lock().await;
        if guard.executing {
            // Should not happen in correct usage.
            return false;
        }
        guard.executing = true;
        true
    }

    /// Called at the end of task execution. Clears `executing=false`.
    /// Returns true if the node is still pending (caller should re-enqueue).
    pub(crate) async fn finish_execute(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> bool {
        {
            let state_arc = self.get_or_create(key, topology);
            let mut guard = state_arc.lock().await;
            guard.executing = false;
        }
        // Re-check pending (a dirty signal may have arrived during execution).
        self.is_pending(key, topology).await
    }

    // -----------------------------------------------------------------------
    // Slot state mutations
    // -----------------------------------------------------------------------

    /// Mark an output slot as dirty (value changed or newly available).
    pub(crate) async fn mark_dirty(
        &self,
        key: NodeInstanceKey,
        slot: SlotIndex,
        topology: &Topology,
    ) {
        let state = self.get_or_create(key, topology);
        let mut guard = state.lock().await;
        if let Some(ss) = guard.output_slots.get_mut(slot) {
            ss.dirty = true;
        }
    }

    /// Mark an output slot as cleanly present with a new hash.
    pub(crate) async fn mark_present(
        &self,
        key: NodeInstanceKey,
        slot: SlotIndex,
        hash: ValueHash,
        topology: &Topology,
    ) {
        let state = self.get_or_create(key, topology);
        let mut guard = state.lock().await;
        if let Some(ss) = guard.output_slots.get_mut(slot) {
            ss.present = true;
            ss.dirty = false;
            ss.error = false;
            ss.hash = Some(hash);
        }
    }

    /// Mark all output slots of a node instance as error.
    pub(crate) async fn mark_all_error(&self, key: NodeInstanceKey, topology: &Topology) {
        let state = self.get_or_create(key, topology);
        let mut guard = state.lock().await;
        for ss in &mut guard.output_slots {
            ss.error = true;
            ss.dirty = false;
        }
    }

    /// Get the stored hash for an output slot.
    pub(crate) async fn get_hash(
        &self,
        key: NodeInstanceKey,
        slot: SlotIndex,
        topology: &Topology,
    ) -> Option<ValueHash> {
        let state = self.get_or_create(key, topology);
        let guard = state.lock().await;
        guard.output_slots.get(slot).and_then(|s| s.hash)
    }

    /// Update collection element keys for a slot (after a collection output).
    pub(crate) async fn set_collection_keys(
        &self,
        key: NodeInstanceKey,
        slot: SlotIndex,
        keys: Vec<u64>,
        hashes: HashMap<u64, ValueHash>,
        topology: &Topology,
    ) {
        let state = self.get_or_create(key, topology);
        let mut guard = state.lock().await;
        if let Some(ss) = guard.output_slots.get_mut(slot) {
            ss.element_keys = keys;
            ss.element_hashes = hashes;
        }
    }

    /// Get the previous collection element hashes for a slot.
    pub(crate) async fn get_collection_hashes(
        &self,
        key: NodeInstanceKey,
        slot: SlotIndex,
        topology: &Topology,
    ) -> HashMap<u64, ValueHash> {
        let state = self.get_or_create(key, topology);
        let guard = state.lock().await;
        guard.output_slots.get(slot)
            .map(|s| s.element_hashes.clone())
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Snapshot (persistence)
    // -----------------------------------------------------------------------

    /// Serialise the entire WorkState into a snapshot for persistence.
    pub(crate) async fn snapshot(&self) -> WorkStateSnapshot {
        let mut states = vec![];
        for entry in self.node_states.iter() {
            let key = *entry.key();
            let guard = entry.value().lock().await;
            states.push((key, NodeInstanceRecord {
                output_slots: guard.output_slots.clone(),
                execution_count: guard.execution_count,
            }));
        }
        let mut instances = vec![];
        for entry in self.instances.iter() {
            let key = *entry.key();
            let list = entry.value().lock().await.clone();
            instances.push((key, list));
        }
        WorkStateSnapshot { version: 1, states, instances }
    }

    /// Restore from a snapshot. Nodes not in the snapshot start as dirty.
    pub(crate) async fn restore(&self, snapshot: WorkStateSnapshot, topology: &Topology) {
        for (key, rec) in snapshot.states {
            let n_out = topology.node(key.node).output_slots.len();
            let mut nis = NodeInstanceState::new(n_out);
            // Only restore slots that are still valid in current topology.
            for (i, ss) in rec.output_slots.into_iter().enumerate() {
                if i < nis.output_slots.len() {
                    nis.output_slots[i] = ss;
                }
            }
            nis.execution_count = rec.execution_count;
            self.node_states.insert(key, Arc::new(Mutex::new(nis)));
        }
        for (key, list) in snapshot.instances {
            self.instances.insert(key, Arc::new(Mutex::new(list)));
        }
    }

    /// Mark all nodes in the topology as dirty (cold start).
    pub(crate) fn init_cold(&self, topology: &Topology) {
        // Root subgraph: ensure root I/O input nodes have dirty output slots.
        // Other nodes will be lazily created as dirty by get_or_create.
        // We just ensure the root subgraph's unit instance exists.
        for sg in &topology.subgraphs {
            if sg.parent.is_some() { continue; } // only root
            for &nid in &sg.io_input_nodes {
                let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, nid);
                let n_out = topology.node(nid).output_slots.len();
                let state = NodeInstanceState::new(n_out);
                self.node_states.insert(key, Arc::new(Mutex::new(state)));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot types (for persistence)
// ---------------------------------------------------------------------------

/// Serialisable snapshot of [`WorkState`].
#[derive(Serialize, Deserialize)]
pub(crate) struct WorkStateSnapshot {
    pub(crate) version: u32,
    pub(crate) states: Vec<(NodeInstanceKey, NodeInstanceRecord)>,
    pub(crate) instances: Vec<(SubgraphInstanceKey, Vec<InstanceKey>)>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct NodeInstanceRecord {
    pub(crate) output_slots: Vec<SlotState>,
    pub(crate) execution_count: u32,
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Determine the source instance key for a given edge and destination instance.
///
/// - For intra-scope edges: source instance = same as destination instance.
/// - For cross-scope edges from ancestor: source instance = parent instance.
/// - For SubgraphBoundary edges: source is in the parent scope.
pub(crate) fn source_instance_for_edge(
    edge: &crate::topology::EdgeDesc,
    dest_instance: InstanceKey,
    topology: &Topology,
) -> InstanceKey {
    let from_sg = topology.node(edge.from_node).subgraph;
    let to_sg   = topology.node(edge.to_node).subgraph;
    if from_sg == to_sg {
        dest_instance
    } else {
        // Cross-scope: the source is in an ancestor scope. The ancestor's
        // instance key for the root subgraph is always UNIT_INSTANCE.
        // For deeper nesting, the parent instance key is the element key
        // that was used to create the current subgraph instance.
        // In the current design (single level of fan-out common case),
        // this is UNIT_INSTANCE for the root parent.
        // TODO: support deeper nesting by tracking parent instance key per instance.
        UNIT_INSTANCE
    }
}
