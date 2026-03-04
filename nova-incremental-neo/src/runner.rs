//! Execution coordination: `execute_task`, `propagate_dirty`, `propagate_error`,
//! and collection change handling.
//!
//! This module is the only place where `Topology`, `WorkState`, `ValueStore`,
//! `TaskQueue`, and `TransformContext` are combined. All other modules are
//! independently testable.

use std::collections::HashMap;
use std::sync::Arc;

use crate::keys::{
    SubgraphId, InstanceKey, NodeId, SlotIndex, NodeInstanceKey, ElementKey, SlotStateKey,
    UNIT_INSTANCE,
};
use crate::report::UpdateReport;
use crate::task_queue::{ArcSequentialTaskQueue, TaskFn, TaskQueue};
use crate::topology::{Topology, EdgeKind, NodeKind};
use crate::transform::{
    TransformContext, ContextInput, ContextOutput, ErasedValue,
    TransformError,
};
use crate::value_store::ValueStore;
use crate::workstate::{WorkState, ValueHash, source_instance_for_edge};

// ---------------------------------------------------------------------------
// RunContext — shared references passed to all runner functions
// ---------------------------------------------------------------------------

/// Shared read-only references used throughout the runner.
pub(crate) struct RunContext {
    pub(crate) topology:    Arc<Topology>,
    pub(crate) workstate:   Arc<WorkState>,
    pub(crate) value_store: Arc<ValueStore>,
    pub(crate) task_queue:  Arc<ArcSequentialTaskQueue>,
    pub(crate) cycle_limit: u32,
    /// Uses std::sync::Mutex (not tokio::sync::Mutex) so the future remains Send.
    pub(crate) report:      Arc<std::sync::Mutex<UpdateReport>>,
}

impl Clone for RunContext {
    fn clone(&self) -> Self {
        Self {
            topology:    Arc::clone(&self.topology),
            workstate:   Arc::clone(&self.workstate),
            value_store: Arc::clone(&self.value_store),
            task_queue:  Arc::clone(&self.task_queue),
            cycle_limit: self.cycle_limit,
            report:      Arc::clone(&self.report),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry: seed initial tasks from dirty I/O input nodes
// ---------------------------------------------------------------------------

/// Enqueue tasks for all nodes that are pending at the start of `update()`.
///
/// Walks topo order of the root subgraph and enqueues any node that is pending.
/// Child subgraphs are entered when their parent fan-out edges are dirty.
pub(crate) async fn seed_pending_tasks(ctx: &RunContext) {
    let root = ctx.topology.root();
    for &nid in &root.topo_order {
        let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, nid);
        if ctx.workstate.is_pending(key, &ctx.topology).await {
            try_enqueue_task(key, ctx).await;
        }
    }
}

// ---------------------------------------------------------------------------
// set_input: write external value to I/O input node
// ---------------------------------------------------------------------------

/// Write a new value into an I/O input node. Serialises, hashes, caches,
/// updates WorkState, and triggers dirty propagation.
pub(crate) async fn set_input_value(
    node_id: NodeId,
    erased: ErasedValue,
    bytes: Vec<u8>,
    hash: ValueHash,
    type_name: &'static str,
    ctx: &RunContext,
) {
    let key = NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, node_id);
    let slot_key = key.slot_key(0);

    // Check if value changed.
    let old_hash = ctx.workstate.get_hash(key, 0, &ctx.topology).await;
    if old_hash == Some(hash) { return; } // unchanged

    // Update cache and state.
    ctx.value_store.set_erased(slot_key, Arc::clone(&erased), bytes, hash, type_name);
    ctx.workstate.mark_present(key, 0, hash, &ctx.topology).await;
    ctx.workstate.mark_dirty(key, 0, &ctx.topology).await;

    // Propagate to downstream nodes.
    propagate_dirty(key, 0, ctx).await;
}

// ---------------------------------------------------------------------------
// propagate_dirty
// ---------------------------------------------------------------------------

/// Called after an output slot's value changes. Checks all downstream nodes
/// and enqueues them if they are now pending.
pub(crate) async fn propagate_dirty(
    src: NodeInstanceKey,
    slot: SlotIndex,
    ctx: &RunContext,
) {
    let outgoing = ctx.topology.outgoing_edges(src.node, slot);
    for &edge_id in outgoing {
        let edge = ctx.topology.edge(edge_id);

        match &edge.kind {
            EdgeKind::SubgraphBoundary { child } => {
                // Fan-out: the source slot is a collection; push element changes.
                handle_fan_out_change(*child, src.instance, src.node, slot, ctx).await;
            }
            EdgeKind::Single | EdgeKind::Collection => {
                let dest_sg = ctx.topology.node(edge.to_node).subgraph;
                // Determine which instances of the destination need updating.
                let dest_instances = instances_for_dest(
                    dest_sg, src.subgraph, src.instance, ctx
                ).await;
                for dest_instance in dest_instances {
                    let dest_key = NodeInstanceKey::new(dest_sg, dest_instance, edge.to_node);
                    // Mark the destination's relevant output slot as dirty via
                    // its source slot (which is now dirty — already done for src).
                    if ctx.workstate.is_pending(dest_key, &ctx.topology).await {
                        try_enqueue_task(dest_key, ctx).await;
                    }
                }
            }
        }
    }
}

/// Determines the set of instances at `dest_sg` that should be re-evaluated
/// when `src_instance` in `src_sg` changes.
async fn instances_for_dest(
    dest_sg: SubgraphId,
    src_sg: SubgraphId,
    src_instance: InstanceKey,
    ctx: &RunContext,
) -> Vec<InstanceKey> {
    if dest_sg == src_sg {
        return vec![src_instance];
    }
    // Ancestor changed → all child instances must re-evaluate.
    ctx.workstate.instance_keys(dest_sg, src_instance).await
}

// ---------------------------------------------------------------------------
// handle_fan_out_change: manage child subgraph instances
// ---------------------------------------------------------------------------

/// Called when a collection output slot feeding a subgraph boundary changes.
/// Diffs old vs new element keys and creates/removes/re-triggers instances.
async fn handle_fan_out_change(
    child_sg: SubgraphId,
    parent_instance: InstanceKey,
    src_node: NodeId,
    src_slot: SlotIndex,
    ctx: &RunContext,
) {
    let slot_key = SlotStateKey::new(
        ctx.topology.node(src_node).subgraph,
        parent_instance,
        src_node,
        src_slot,
    );

    // Get current element keys from WorkState.
    let src_node_key = NodeInstanceKey::new(
        ctx.topology.node(src_node).subgraph,
        parent_instance,
        src_node,
    );
    let old_hashes = ctx.workstate.get_collection_hashes(src_node_key, src_slot, &ctx.topology).await;
    let old_keys: std::collections::HashSet<u64> = old_hashes.keys().copied().collect();

    // Get new element keys from ValueStore.
    let new_elements = ctx.value_store.get_all_elements_erased(
        slot_key,
        &old_keys.iter().copied().collect::<Vec<_>>(),
    );
    // TODO: need the actual new element keys from the collection output.
    // For now, collect them from what's in the ValueStore for this slot.
    // The runner will need to be called with the actual new element list after
    // a transform writes a collection output.

    // This function is called from propagate_dirty after a collection slot changes.
    // The new elements are already in ValueStore. We re-read them.
    // New keys = all element keys currently in the ValueStore for this slot.
    let new_elements_with_hashes: Vec<(u64, ValueHash)> = {
        let mut result = vec![];
        // Walk the known keys from workstate (updated after transform wrote output).
        let state_arc = ctx.workstate.get_or_create(src_node_key, &ctx.topology);
        let guard = state_arc.lock().await;
        for &ek in &guard.output_slots[src_slot].element_keys {
            if let Some((_, h)) = ctx.value_store.get_element_erased(ElementKey::new(slot_key, ek)) {
                result.push((ek, h));
            }
        }
        result
    };

    let new_keys: std::collections::HashSet<u64> =
        new_elements_with_hashes.iter().map(|(k, _)| *k).collect();

    // Added elements.
    for &ek in new_keys.difference(&old_keys) {
        ctx.workstate.add_instance(child_sg, parent_instance, InstanceKey(ek)).await;
        let root_nid = child_subgraph_root(child_sg, &ctx.topology);
        if let Some(root) = root_nid {
            let root_key = NodeInstanceKey::new(child_sg, InstanceKey(ek), root);
            try_enqueue_task(root_key, ctx).await;
        }
    }

    // Removed elements.
    for &ek in old_keys.difference(&new_keys) {
        ctx.workstate.remove_instance(child_sg, parent_instance, InstanceKey(ek), &ctx.topology).await;
        // Propagate removal to parent's collection outputs.
        propagate_instance_removal(child_sg, InstanceKey(ek), parent_instance, ctx).await;
    }

    // Changed elements (same key, different hash).
    for &ek in new_keys.intersection(&old_keys) {
        let old_hash = old_hashes.get(&ek).copied();
        let new_hash = new_elements_with_hashes.iter()
            .find(|(k, _)| *k == ek)
            .map(|(_, h)| *h);
        if old_hash != new_hash {
            let root_nid = child_subgraph_root(child_sg, &ctx.topology);
            if let Some(root) = root_nid {
                let root_key = NodeInstanceKey::new(child_sg, InstanceKey(ek), root);
                // Mark the boundary input slot dirty for this instance.
                ctx.workstate.mark_dirty(root_key, 0, &ctx.topology).await;
                try_enqueue_task(root_key, ctx).await;
            }
        }
    }
}

/// Finds the root node of a child subgraph (the node that receives the fan-out edge).
fn child_subgraph_root(child_sg: SubgraphId, topology: &Topology) -> Option<NodeId> {
    let sg_desc = &topology.subgraphs[child_sg.0 as usize];
    let boundary_edge_id = sg_desc.collection_input_edge?;
    let edge = topology.edge(boundary_edge_id);
    Some(edge.to_node)
}

/// Propagates the removal of an instance to parent-scope collection outputs.
async fn propagate_instance_removal(
    child_sg: SubgraphId,
    instance: InstanceKey,
    parent_instance: InstanceKey,
    ctx: &RunContext,
) {
    // For each output edge from the child subgraph to the parent, mark the
    // parent's downstream nodes as pending.
    let sg_desc = &ctx.topology.subgraphs[child_sg.0 as usize];
    let parent_sg = sg_desc.parent.unwrap_or(SubgraphId(0));

    for &nid in &sg_desc.topo_order {
        let node_key = NodeInstanceKey::new(child_sg, instance, nid);
        let node_desc = ctx.topology.node(nid);
        for slot_idx in 0..node_desc.output_slots.len() {
            let outgoing = ctx.topology.outgoing_edges(nid, slot_idx);
            for &edge_id in outgoing {
                let edge = ctx.topology.edge(edge_id);
                let dest_sg = ctx.topology.node(edge.to_node).subgraph;
                if dest_sg == parent_sg {
                    // This is a subgraph output edge. Mark parent node dirty.
                    let dest_key = NodeInstanceKey::new(parent_sg, parent_instance, edge.to_node);
                    ctx.workstate.mark_dirty(dest_key, edge.to_slot, &ctx.topology).await;
                    if ctx.workstate.is_pending(dest_key, &ctx.topology).await {
                        try_enqueue_task(dest_key, ctx).await;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// propagate_error
// ---------------------------------------------------------------------------

/// Called when a transform fails. Marks all downstream nodes as blocked.
/// Uses an iterative BFS (not recursive) to avoid non-Send futures.
pub(crate) async fn propagate_error(
    src: NodeInstanceKey,
    ctx: &RunContext,
) {
    ctx.workstate.mark_all_error(src, &ctx.topology).await;
    let mut queue: std::collections::VecDeque<NodeInstanceKey> = std::collections::VecDeque::new();
    queue.push_back(src);
    while let Some(cur) = queue.pop_front() {
        let node_desc = ctx.topology.node(cur.node);
        for slot_idx in 0..node_desc.output_slots.len() {
            let outgoing = ctx.topology.outgoing_edges(cur.node, slot_idx);
            for &edge_id in outgoing {
                let edge = ctx.topology.edge(edge_id);
                let dest_sg = ctx.topology.node(edge.to_node).subgraph;
                let dest_key = NodeInstanceKey::new(dest_sg, cur.instance, edge.to_node);
                ctx.workstate.mark_all_error(dest_key, &ctx.topology).await;
                queue.push_back(dest_key);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// try_enqueue_task
// ---------------------------------------------------------------------------

/// Checks WorkState deduplication and pushes a task closure onto the queue.
pub(crate) async fn try_enqueue_task(key: NodeInstanceKey, ctx: &RunContext) {
    if !ctx.workstate.try_enqueue(key, &ctx.topology).await { return; }
    let ctx2 = ctx.clone();
    let task: TaskFn = Box::new(move || Box::pin(async move {
        execute_task(key, &ctx2).await;
    }));
    ctx.task_queue.enqueue(task);
}

// ---------------------------------------------------------------------------
// execute_task
// ---------------------------------------------------------------------------

/// Execute the transform for one node instance.
pub(crate) async fn execute_task(key: NodeInstanceKey, ctx: &RunContext) {
    // Step 1: claim execution.
    if !ctx.workstate.begin_execute(key, &ctx.topology).await { return; }

    // Step 2: re-check pending.
    if !ctx.workstate.is_pending(key, &ctx.topology).await {
        finish(key, ctx).await;
        return;
    }

    // Step 3: build TransformContext.
    let node_desc = ctx.topology.node(key.node);
    let transform = match &node_desc.kind {
        NodeKind::Transform(t) => Arc::clone(t),
        _ => {
            // I/O nodes don't execute transforms.
            finish(key, ctx).await;
            return;
        }
    };

    let context_result = build_context(key, ctx).await;
    let mut transform_ctx = match context_result {
        Ok(c) => c,
        Err(e) => {
            record_error(key, e, ctx).await;
            finish(key, ctx).await;
            return;
        }
    };

    // Step 4: call apply.
    let apply_result = transform.apply_erased(&mut transform_ctx).await;

    // Step 5: handle result.
    match apply_result {
        Err(e) => {
            record_error(key, e, ctx).await;
        }
        Ok(()) => {
            commit_outputs(key, transform_ctx, ctx).await;
        }
    }

    // Step 6: increment execution count and check cycle limit.
    {
        let state_arc = ctx.workstate.get_or_create(key, &ctx.topology);
        let mut guard = state_arc.lock().await;
        guard.execution_count += 1;
        if guard.execution_count > ctx.cycle_limit {
            let uuid = key.node.as_uuid();
            let mut report = ctx.report.lock().unwrap();
            report.cycle_limit_exceeded.push(uuid);
            // Mark all outputs as error to stop further propagation.
            for ss in &mut guard.output_slots {
                ss.error = true;
                ss.dirty = false;
            }
        }
    }

    finish(key, ctx).await;
}

/// Increment report counters and call finish_execute, re-enqueueing if still pending.
async fn finish(key: NodeInstanceKey, ctx: &RunContext) {
    let still_pending = ctx.workstate.finish_execute(key, &ctx.topology).await;
    if still_pending {
        try_enqueue_task(key, ctx).await;
    }
}

/// Record a transform error in the report and propagate the error flag downstream.
async fn record_error(key: NodeInstanceKey, e: TransformError, ctx: &RunContext) {
    {
        let mut report = ctx.report.lock().unwrap();
        report.errors.push((key.node.as_uuid(), e));
        report.transforms_evaluated += 1;
    }
    propagate_error(key, ctx).await;
}

// ---------------------------------------------------------------------------
// build_context — assemble TransformContext for a node instance
// ---------------------------------------------------------------------------

async fn build_context(
    key: NodeInstanceKey,
    ctx: &RunContext,
) -> Result<TransformContext, TransformError> {
    let node_desc = ctx.topology.node(key.node);
    let mut inputs: Vec<ContextInput> = Vec::with_capacity(node_desc.input_slots.len());

    for (slot_idx, slot_kind) in node_desc.input_slots.iter().enumerate() {
        let Some(edge_id) = ctx.topology.incoming_edge(key.node, slot_idx) else {
            inputs.push(ContextInput::Absent);
            continue;
        };
        let edge = ctx.topology.edge(edge_id);
        let src_instance = source_instance_for_edge(edge, key.instance, &ctx.topology);
        let src_sg = ctx.topology.node(edge.from_node).subgraph;
        let src_key = NodeInstanceKey::new(src_sg, src_instance, edge.from_node);
        let src_slot_key = src_key.slot_key(edge.from_slot);

        if slot_kind.is_collection {
            // Gather: collect all elements for this slot.
            let prev_hashes = ctx.workstate
                .get_collection_hashes(src_key, edge.from_slot, &ctx.topology).await;
            let state_arc = ctx.workstate.get_or_create(src_key, &ctx.topology);
            let element_keys = {
                let guard = state_arc.lock().await;
                guard.output_slots.get(edge.from_slot)
                    .map(|s| s.element_keys.clone())
                    .unwrap_or_default()
            };
            let all_elems = ctx.value_store.get_all_elements_erased(src_slot_key, &element_keys);
            let mut elements: Vec<ErasedValue> = Vec::with_capacity(all_elems.len());
            let mut keys: Vec<u64>     = Vec::with_capacity(all_elems.len());
            let mut dirty_keys: Vec<u64>  = vec![];
            let mut removed_keys: Vec<u64> = vec![];

            for (ek, v, h) in all_elems {
                elements.push(v);
                keys.push(ek);
                let old = prev_hashes.get(&ek).copied();
                if old.is_none() || old != Some(h) {
                    dirty_keys.push(ek);
                }
            }
            for &old_key in prev_hashes.keys() {
                if !keys.contains(&old_key) {
                    removed_keys.push(old_key);
                }
            }

            inputs.push(ContextInput::Collection {
                elements,
                keys,
                dirty_keys,
                removed_keys,
            });
        } else {
            // Single value.
            match ctx.value_store.get_erased(src_slot_key) {
                Some((v, _)) => inputs.push(ContextInput::Single(v)),
                None => inputs.push(ContextInput::Absent),
            }
        }
    }

    Ok(TransformContext::new(
        inputs,
        node_desc.input_slots.clone(),
        node_desc.output_slots.clone(),
    ))
}

// ---------------------------------------------------------------------------
// commit_outputs — write transform outputs back to WorkState + ValueStore
// ---------------------------------------------------------------------------

async fn commit_outputs(
    key: NodeInstanceKey,
    mut transform_ctx: TransformContext,
    ctx: &RunContext,
) {
    let node_desc = ctx.topology.node(key.node);
    let mut changed_any = false;

    for (slot_idx, output) in transform_ctx.outputs.drain(..).enumerate() {
        let slot_key = key.slot_key(slot_idx);
        let slot_kind = &node_desc.output_slots[slot_idx];

        match output {
            ContextOutput::Single(v) => {
                // Serialize + hash.
                let (bytes, hash) = match erased_to_bytes_hash(&v) {
                    Ok(r) => r,
                    Err(e) => {
                        record_error(key, TransformError::new(e), ctx).await;
                        continue;
                    }
                };
                let old_hash = ctx.workstate.get_hash(key, slot_idx, &ctx.topology).await;
                if old_hash != Some(hash) {
                    ctx.value_store.set_erased(slot_key, v, bytes, hash, slot_kind.type_name);
                    ctx.workstate.mark_present(key, slot_idx, hash, &ctx.topology).await;
                    changed_any = true;
                    propagate_dirty(key, slot_idx, ctx).await;
                } else {
                    ctx.workstate.mark_present(key, slot_idx, hash, &ctx.topology).await;
                }
            }
            ContextOutput::Collection(pairs) => {
                // Compute diff vs previous element_hashes.
                let prev_hashes = ctx.workstate
                    .get_collection_hashes(key, slot_idx, &ctx.topology).await;
                let mut new_keys: Vec<u64> = vec![];
                let mut new_hashes: HashMap<u64, ValueHash> = HashMap::new();
                let mut any_element_changed = false;

                for (ek, v) in &pairs {
                    let (bytes, hash) = match erased_to_bytes_hash(v) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    new_keys.push(*ek);
                    new_hashes.insert(*ek, hash);
                    let elem_key = ElementKey::new(slot_key, *ek);
                    let old_hash = prev_hashes.get(ek).copied();
                    if old_hash != Some(hash) {
                        ctx.value_store.set_element_erased(
                            elem_key, Arc::clone(v), bytes, hash, slot_kind.type_name
                        );
                        any_element_changed = true;
                    }
                }
                // Check for removed elements.
                for &old_ek in prev_hashes.keys() {
                    if !new_keys.contains(&old_ek) {
                        ctx.value_store.remove_element(ElementKey::new(slot_key, old_ek));
                        any_element_changed = true;
                    }
                }

                new_keys.sort_unstable();
                ctx.workstate.set_collection_keys(
                    key, slot_idx, new_keys, new_hashes, &ctx.topology
                ).await;

                let agg_hash: ValueHash = {
                    let mut h: ValueHash = 0;
                    for (_, hash) in ctx.workstate
                        .get_collection_hashes(key, slot_idx, &ctx.topology).await {
                        h ^= hash; // XOR for order-independent aggregate hash
                    }
                    h
                };
                ctx.workstate.mark_present(key, slot_idx, agg_hash, &ctx.topology).await;

                if any_element_changed {
                    changed_any = true;
                    propagate_dirty(key, slot_idx, ctx).await;
                }
            }
            ContextOutput::Absent => {
                // Transform did not write to this slot; leave state unchanged.
            }
        }
    }

    {
        let mut report = ctx.report.lock().unwrap();
        report.transforms_evaluated += 1;
        if changed_any {
            report.transforms_changed += 1;
        } else {
            report.transforms_skipped += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: serialize ErasedValue to bytes + hash
// ---------------------------------------------------------------------------

/// Serialise a type-erased value to msgpack bytes and compute its hash.
///
/// The caller must ensure the value was boxed as a type implementing
/// `serde::Serialize`. Since we stored it as `Arc<T>` where `T: IncrementalValue`,
/// we need to use a vtable approach.
///
/// CHANGES: This requires the ErasedValue to carry a serialize fn pointer.
/// For the initial implementation we return an error; see CHANGES.md.
fn erased_to_bytes_hash(_v: &ErasedValue) -> Result<(Vec<u8>, ValueHash), String> {
    // TODO: ErasedValue needs to carry serialization capability.
    // This is tracked as CHANGES.md item #2.
    // For now we produce a deterministic placeholder hash based on the pointer.
    // Cast fat pointer to thin pointer then to u64 (fat pointers cannot cast directly).
    let ptr = Arc::as_ptr(_v) as *const () as u64;
    let bytes = ptr.to_le_bytes().to_vec();
    Ok((bytes, ptr))
}
