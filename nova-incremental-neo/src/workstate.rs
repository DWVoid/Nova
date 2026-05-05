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
    /// Persisted so that incremental diff works correctly across warm starts.
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
    /// Set to true when `try_enqueue` is called while `executing=true`.
    pub(crate) re_enqueue_requested: bool,
    /// Set by `propagate_instance_removal` to bypass the is_pending re-check
    /// in execute_task step 2 (needed for cross-scope gather after child removal).
    pub(crate) force_execute: bool,
}

impl NodeInstanceState {
    fn new(n_output_slots: usize) -> Self {
        Self {
            output_slots: (0..n_output_slots).map(|_| SlotState::new_dirty()).collect(),
            execution_count: 0,
            executing: false,
            re_enqueue_requested: false,
            force_execute: false,
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
// InstanceAncestry — tracks parent instance relationships for multi-level nesting
// ---------------------------------------------------------------------------

/// Information about an instance's parent in the subgraph hierarchy.
/// Used to resolve cross-scope edges in deeply nested fan-out scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InstanceAncestry {
    /// The subgraph in which this instance was created (the child subgraph).
    pub(crate) subgraph: SubgraphId,
    /// The instance key within the parent subgraph that created this instance.
    /// For the root subgraph, this is UNIT_INSTANCE.
    pub(crate) parent_instance: InstanceKey,
    /// The element key that identifies this instance within the parent's collection.
    pub(crate) element_key: InstanceKey,
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
    /// Ancestry info per (SubgraphId, InstanceKey) — tracks parent for cross-scope resolution.
    ancestry: DashMap<(SubgraphId, InstanceKey), InstanceAncestry>,
    /// Tasks currently in the queue but not yet executing.
    /// Uses `std::sync::Mutex` so `try_enqueue` is sync (breaking async call cycles).
    enqueued: std::sync::Mutex<HashSet<NodeInstanceKey>>,
}

impl WorkState {
    pub(crate) fn new() -> Self {
        Self {
            node_states: DashMap::new(),
            instances:   DashMap::new(),
            ancestry:    DashMap::new(),
            enqueued:    std::sync::Mutex::new(HashSet::new()),
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

    /// Register a new child instance with ancestry tracking.
    ///
    /// Records both the instance membership (for enumeration) and the ancestry
    /// info (for cross-scope edge resolution in deeply nested scenarios).
    pub(crate) async fn add_instance(
        &self,
        subgraph: SubgraphId,
        parent_instance: InstanceKey,
        element_key: InstanceKey,
    ) {
        // Register in the instances map for enumeration.
        let key = SubgraphInstanceKey::new(subgraph, parent_instance);
        let entry = self.instances.entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(vec![])));
        let mut list = entry.lock().await;
        if !list.contains(&element_key) {
            list.push(element_key);
            list.sort_unstable();
        }
        
        // Record ancestry info for cross-scope edge resolution.
        // The ancestry is stored keyed by (subgraph, element_key) so we can look up
        // the parent when resolving cross-scope edges.
        let ancestry = InstanceAncestry {
            subgraph,
            parent_instance,
            element_key,
        };
        self.ancestry.insert((subgraph, element_key), ancestry);
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
        // Clean up ancestry info.
        self.ancestry.remove(&(subgraph, element_key));
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

    /// Resolve the source instance for a cross-scope edge.
    /// Wraps the free function to provide access to ancestry data.
    pub(crate) fn resolve_source_instance(
        &self,
        edge: &crate::topology::EdgeDesc,
        dest_instance: InstanceKey,
        topology: &Topology,
    ) -> InstanceKey {
        source_instance_for_edge(edge, dest_instance, topology, &self.ancestry)
    }

    // -----------------------------------------------------------------------
    // Readiness queries
    // -----------------------------------------------------------------------

    /// Returns true if all required inputs for `key` are present and error-free.
    ///
    /// For each input slot, resolves all source output slots via topology and
    /// checks their [`SlotState`]. Back-edges do not block readiness.
    /// For cross-scope single→collection edges (subgraph output gather),
    /// readiness is satisfied if at least one child instance has produced output.
    pub(crate) async fn is_ready(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> bool {
        let node_desc = topology.node(key.node);
        for (slot_idx, _) in node_desc.input_slots.iter().enumerate() {
            let edge_ids = topology.incoming_edges(key.node, slot_idx);
            if edge_ids.is_empty() {
                return false;
            }
            for &edge_id in edge_ids {
                let edge = topology.edge(edge_id);
                if edge.is_back_edge { continue; }

                let src_sg = topology.node(edge.from_node).subgraph;
                if src_sg != key.subgraph && !topology.node(edge.from_node).output_is_collection(edge.from_slot) {
                    let child_instances = self.instance_keys(src_sg, key.instance).await;
                    if child_instances.is_empty() {
                        return false;
                    }
                    let mut any_present = false;
                    for &inst in &child_instances {
                        let src_key = NodeInstanceKey::new(src_sg, inst, edge.from_node);
                        if let Some(state_arc) = self.node_states.get(&src_key) {
                            let guard = state_arc.lock().await;
                            if guard.output_slots[edge.from_slot].present && !guard.output_slots[edge.from_slot].error {
                                any_present = true;
                                break;
                            }
                        }
                    }
                    if !any_present {
                        return false;
                    }
                } else {
                    let src_instance = self.resolve_source_instance(edge, key.instance, topology);
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
            }
        }
        true
    }

    /// Returns true if `is_ready` and at least one non-back-edge input is dirty.
    pub(crate) async fn is_pending(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> bool {
        if !self.is_ready(key, topology).await { return false; }
        let node_desc = topology.node(key.node);
        for (slot_idx, _) in node_desc.input_slots.iter().enumerate() {
            let edge_ids = topology.incoming_edges(key.node, slot_idx);
            for &edge_id in edge_ids {
                let edge = topology.edge(edge_id);
                if edge.is_back_edge { continue; }

                let src_sg = topology.node(edge.from_node).subgraph;
                if src_sg != key.subgraph && !topology.node(edge.from_node).output_is_collection(edge.from_slot) {
                    // Cross-scope single→collection gather: check any child instance dirty.
                    let child_instances = self.instance_keys(src_sg, key.instance).await;
                    for &inst in &child_instances {
                        let src_key = NodeInstanceKey::new(src_sg, inst, edge.from_node);
                        if let Some(state_arc) = self.node_states.get(&src_key) {
                            let guard = state_arc.lock().await;
                            if guard.output_slots[edge.from_slot].dirty { return true; }
                        }
                    }
                } else {
                    let src_instance = self.resolve_source_instance(edge, key.instance, topology);
                    let src_key = NodeInstanceKey::new(
                        topology.node(edge.from_node).subgraph,
                        src_instance,
                        edge.from_node,
                    );
                    let src_state = self.get_or_create(src_key, topology);
                    let guard = src_state.lock().await;
                    if guard.output_slots[edge.from_slot].dirty { return true; }
                }
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Enqueue / execute protocol
    // -----------------------------------------------------------------------

    /// Try to enqueue `key`. Returns `true` if successfully added to the queue.
    /// Returns `false` if already enqueued or currently executing.
    ///
    /// This is a **synchronous** function — the `enqueued` set uses
    /// `std::sync::Mutex`, which breaks the async call cycle between
    /// `try_enqueue_task` and `execute_task`.
    pub(crate) fn try_enqueue(
        &self,
        key: NodeInstanceKey,
        _topology: &Topology,
    ) -> bool {
        let mut enqueued = self.enqueued.lock().unwrap();
        if enqueued.contains(&key) { return false; }
        if let Some(state_arc) = self.node_states.get(&key) {
            if let Ok(mut guard) = state_arc.try_lock() {
                if guard.executing {
                    guard.re_enqueue_requested = true;
                    return false;
                }
            } else {
                // Lock is held — task is executing. Set flag to request re-enqueue.
                // We can't acquire the lock to set the flag, but the next
                // finish_execute will check via is_pending to catch this case.
                // Use a best-effort approach: mark a
                // separate atomic flag on the WorkState level.
                // For now, rely on the executing check + finish_execute re-check.
                return false;
            }
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
            self.enqueued.lock().unwrap().remove(&key);
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
    /// Returns true if a re-enqueue was requested during execution
    /// (caller should re-enqueue).
    pub(crate) async fn finish_execute(
        &self,
        key: NodeInstanceKey,
        topology: &Topology,
    ) -> bool {
        let re_enqueue;
        {
            let state_arc = self.get_or_create(key, topology);
            let mut guard = state_arc.lock().await;
            re_enqueue = guard.re_enqueue_requested;
            guard.re_enqueue_requested = false;
            guard.executing = false;
        }
        re_enqueue
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

    /// Mark an output slot as present with a new hash.
    /// Does NOT clear the dirty flag — dirty is managed by [`mark_dirty`]
    /// and [`reset_cycle`], not by mark_present.
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
            ss.error = false;
            ss.hash = Some(hash);
        }
    }

    /// Reset per-cycle state: clear dirty flags and execution counts.
    /// Called at the start of [`update()`](crate::engine::Engine::update).
    pub(crate) fn reset_cycle(&self) {
        for entry in self.node_states.iter() {
            if let Ok(mut guard) = entry.value().try_lock() {
                for ss in &mut guard.output_slots {
                    ss.dirty = false;
                }
                guard.execution_count = 0;
            }
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
        topology: &Topology,
    ) {
        let state = self.get_or_create(key, topology);
        let mut guard = state.lock().await;
        if let Some(ss) = guard.output_slots.get_mut(slot) {
            ss.element_keys = keys;
        }
    }

    /// Update element hashes after propagate_dirty has consumed the old ones.
    pub(crate) async fn update_collection_hashes(
        &self,
        key: NodeInstanceKey,
        slot: SlotIndex,
        hashes: HashMap<u64, ValueHash>,
        topology: &Topology,
    ) {
        let state = self.get_or_create(key, topology);
        let mut guard = state.lock().await;
        if let Some(ss) = guard.output_slots.get_mut(slot) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::topology::TopologyBuilder;
    use crate::transform::{Transform, TransformContext, TransformRegisterContext, TransformError};
    use async_trait::async_trait;

    struct Noop;
    #[async_trait]
    impl Transform for Noop {
        fn register(ctx: &mut impl TransformRegisterContext) {
            ctx.input::<u64>();
            ctx.output::<u64>();
        }
        async fn apply(&self, _ctx: &mut TransformContext) -> Result<(), TransformError> { Ok(()) }
    }

    fn uid(n: u8) -> NodeId { NodeId::from_uuid(Uuid::from_u128(n as u128)) }

    /// Build a minimal topology: input(1) → transform(2) → output(3)
    fn simple_topo() -> Topology {
        let mut b = TopologyBuilder::new();
        b.register_transform("n", Noop);
        b.add_io_input(Uuid::from_u128(1));
        b.add_transform_node(Uuid::from_u128(2), "n");
        b.add_io_output(Uuid::from_u128(3));
        b.add_edge(Uuid::from_u128(1), 0, Uuid::from_u128(2), 0);
        b.add_edge(Uuid::from_u128(2), 0, Uuid::from_u128(3), 0);
        b.freeze().unwrap()
    }

    // ------------------------------------------------------------------
    // NodeInstanceState
    // ------------------------------------------------------------------

    #[test]
    fn test_node_instance_state_created_dirty() {
        let state = NodeInstanceState::new(2);
        assert_eq!(state.output_slots.len(), 2);
        for slot in &state.output_slots {
            assert!(slot.dirty, "output slots should start dirty");
            assert!(!slot.present);
            assert!(!slot.error);
            assert!(slot.hash.is_none());
        }
        assert_eq!(state.execution_count, 0);
        assert!(!state.executing);
    }

    #[test]
    fn test_all_outputs_present_and_ok() {
        let mut state = NodeInstanceState::new(2);
        // All start absent → not present
        assert!(!state.all_outputs_present_and_ok());
        state.output_slots[0].present = true;
        assert!(!state.all_outputs_present_and_ok());
        state.output_slots[1].present = true;
        assert!(state.all_outputs_present_and_ok());
        state.output_slots[1].error = true;
        assert!(!state.all_outputs_present_and_ok());
    }

    #[test]
    fn test_any_output_dirty() {
        let mut state = NodeInstanceState::new(2);
        assert!(state.any_output_dirty());
        state.output_slots[0].dirty = false;
        state.output_slots[1].dirty = false;
        assert!(!state.any_output_dirty());
        state.output_slots[0].dirty = true;
        assert!(state.any_output_dirty());
    }

    // ------------------------------------------------------------------
    // WorkState creation
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_or_create_node_state() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, NodeId::from_uuid(Uuid::from_u128(2)));

        let s1 = ws.get_or_create(key, &topo);
        let s2 = ws.get_or_create(key, &topo);
        // Same Arc
        assert!(Arc::ptr_eq(&s1, &s2));
    }

    // ------------------------------------------------------------------
    // mark_present does NOT clear dirty
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_mark_present_does_not_clear_dirty() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, uid(2));

        // Initially dirty
        let s = ws.get_or_create(key, &topo);
        assert!(s.lock().await.output_slots[0].dirty);

        // mark_present should keep dirty=true
        ws.mark_present(key, 0, 42, &topo).await;
        assert!(s.lock().await.output_slots[0].present);
        assert!(!s.lock().await.output_slots[0].error);
        assert_eq!(s.lock().await.output_slots[0].hash, Some(42));
        // dirty should still be true
        assert!(s.lock().await.output_slots[0].dirty);
    }

    // ------------------------------------------------------------------
    // mark_dirty
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_mark_dirty() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, uid(2));

        // Clear dirty first
        let s = ws.get_or_create(key, &topo);
        s.lock().await.output_slots[0].dirty = false;
        assert!(!s.lock().await.output_slots[0].dirty);

        ws.mark_dirty(key, 0, &topo).await;
        assert!(s.lock().await.output_slots[0].dirty);
    }

    // ------------------------------------------------------------------
    // reset_cycle clears dirty and execution_count
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_reset_cycle_clears_dirty() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, uid(2));

        let s = ws.get_or_create(key, &topo);
        s.lock().await.output_slots[0].dirty = true;
        s.lock().await.execution_count = 5;

        ws.reset_cycle();

        assert!(!s.lock().await.output_slots[0].dirty);
        assert_eq!(s.lock().await.execution_count, 0);
    }

    // ------------------------------------------------------------------
    // Instance lifecycle
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_instance_add_list() {
        let ws = WorkState::new();

        ws.add_instance(SubgraphId(1), UNIT_INSTANCE, InstanceKey(10)).await;
        let keys = ws.instance_keys(SubgraphId(1), UNIT_INSTANCE).await;
        assert_eq!(keys, vec![InstanceKey(10)]);

        ws.add_instance(SubgraphId(1), UNIT_INSTANCE, InstanceKey(20)).await;
        let keys = ws.instance_keys(SubgraphId(1), UNIT_INSTANCE).await;
        assert_eq!(keys, vec![InstanceKey(10), InstanceKey(20)]);

        // Duplicate add should not add a second copy
        ws.add_instance(SubgraphId(1), UNIT_INSTANCE, InstanceKey(10)).await;
        let keys = ws.instance_keys(SubgraphId(1), UNIT_INSTANCE).await;
        assert_eq!(keys.len(), 2);
    }

    // ------------------------------------------------------------------
    // Ancestry tracking
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_ancestry_tracking() {
        let ws = WorkState::new();
        ws.add_instance(SubgraphId(1), UNIT_INSTANCE, InstanceKey(42)).await;
        let entry = ws.ancestry.get(&(SubgraphId(1), InstanceKey(42)));
        assert!(entry.is_some());
        let a = entry.unwrap();
        assert_eq!(a.subgraph, SubgraphId(1));
        assert_eq!(a.parent_instance, UNIT_INSTANCE);
        assert_eq!(a.element_key, InstanceKey(42));
        drop(a);
    }

    // ------------------------------------------------------------------
    // Enqueue/Execute protocol
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_try_enqueue_deduplicates() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, uid(2));

        assert!(ws.try_enqueue(key, &topo));
        assert!(!ws.try_enqueue(key, &topo)); // duplicate
    }

    #[tokio::test]
    async fn test_begin_execute_blocks_reenqueue() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, uid(2));

        ws.try_enqueue(key, &topo);
        assert!(ws.begin_execute(key, &topo).await);
        // While executing, try_enqueue returns false (deduplication)
        assert!(!ws.try_enqueue(key, &topo));
        // After finish_execute, can enqueue again
        ws.finish_execute(key, &topo).await;
        assert!(ws.try_enqueue(key, &topo));
    }

    #[tokio::test]
    async fn test_finish_execute_clears_executing() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, uid(2));

        ws.begin_execute(key, &topo).await;
        let s = ws.get_or_create(key, &topo);
        assert!(s.lock().await.executing);

        ws.finish_execute(key, &topo).await;
        assert!(!s.lock().await.executing);
    }

    // ------------------------------------------------------------------
    // Snapshot / restore round-trip
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_snapshot_restore_roundtrip() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, uid(2));

        // Set some state
        ws.mark_present(key, 0, 123, &topo).await;
        ws.mark_dirty(key, 0, &topo).await;
        {
            let s = ws.get_or_create(key, &topo);
            let mut guard = s.lock().await;
            guard.execution_count = 7;
        }

        // Snapshot
        let snapshot = ws.snapshot().await;

        // New workstate, restore
        let ws2 = WorkState::new();
        ws2.restore(snapshot, &topo).await;

        // Verify restored state
        let s2 = ws2.get_or_create(key, &topo);
        let guard = s2.lock().await;
        assert!(guard.output_slots[0].present);
        assert!(guard.output_slots[0].dirty); // dirty was true
        assert_eq!(guard.output_slots[0].hash, Some(123));
        assert_eq!(guard.execution_count, 7);
    }

    // ------------------------------------------------------------------
    // Multi-edge gather readiness
    // We need two nodes feeding one collection input slot.
    // For simplicity, directly test the workstate logic.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_is_ready_with_no_inputs() {
        let ws = WorkState::new();
        let topo = simple_topo();
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, uid(1));
        // uid(1) is I/O input node with 0 input slots → vacuously ready
        assert!(ws.is_ready(key, &topo).await);
    }
}

/// Determine the source instance key for a given edge and destination instance.
///
/// - For intra-scope edges: source instance = same as destination instance.
/// - For cross-scope edges from ancestor: walk the ancestry chain to find the
///   correct ancestor instance key. Walks up through the subgraph parent chain,
///   looking up each level's ancestry entry to find the parent instance key.
pub(crate) fn source_instance_for_edge(
    edge: &crate::topology::EdgeDesc,
    dest_instance: InstanceKey,
    topology: &Topology,
    ancestry: &DashMap<(SubgraphId, InstanceKey), InstanceAncestry>,
) -> InstanceKey {
    let from_sg = topology.node(edge.from_node).subgraph;
    let to_sg   = topology.node(edge.to_node).subgraph;

    if from_sg == to_sg {
        return dest_instance;
    }

    // Cross-scope: walk the subgraph parent chain to find the correct ancestor instance.
    let mut current_sg = to_sg;
    let mut current_instance = dest_instance;

    // Walk up through the subgraph parent chain.
    // For each level, look up the ancestry info to find the parent instance.
    while current_sg != from_sg {
        let sg = &topology.subgraphs[current_sg.0 as usize];
        match sg.parent {
            Some(parent_sg) => {
                // Try to find ancestry info for (current_sg, current_instance).
                if let Some(ancestry_info) = ancestry.get(&(current_sg, current_instance)) {
                    current_instance = ancestry_info.parent_instance;
                    current_sg = parent_sg;
                } else {
                    // No ancestry info: this instance was not created via fan-out,
                    // meaning it's the unit instance of a previously-seen subgraph.
                    // Fall back to UNIT_INSTANCE for the next parent level.
                    // This should not happen in well-formed graphs.
                    return UNIT_INSTANCE;
                }
            }
            None => {
                // Reached root without finding from_sg.
                break;
            }
        }
    }

    if current_sg == from_sg {
        current_instance
    } else {
        // from_sg is not an ancestor of to_sg (should not happen in validated topology).
        UNIT_INSTANCE
    }
}
