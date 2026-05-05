# nova-incremental-neo — Implemented Architecture

> This document describes the **actually implemented** architecture of the
> next-generation incremental computation engine, reflecting all fixes and
> deviations from the original `DESIGN.md`.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Graph Model](#2-graph-model)
3. [Subgraph Model](#3-subgraph-model)
4. [Build Phase](#4-build-phase)
5. [Storage Layer](#5-storage-layer)
6. [Dirty Flag Protocol](#6-dirty-flag-protocol)
7. [Update Propagation](#7-update-propagation)
8. [Task Execution](#8-task-execution)
9. [Scheduling](#9-scheduling)
10. [Back-Edges and Collector Pattern](#10-back-edges-and-collector-pattern)
11. [Error Handling and Cycle Detection](#11-error-handling-and-cycle-detection)
12. [Checkpoint / Commit / Discard](#12-checkpoint--commit--discard)
13. [Warm Start](#13-warm-start)

---

## 1. Overview

`nova-incremental-neo` is a dataflow incremental computation engine.
Users define a **static graph** of **transform nodes** connected by **edges**.
The engine feeds **input values** through the graph and tracks which outputs
need recomputation when inputs change.

### Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **Edge-driven reactive execution** | Writing to an output slot triggers downstream readiness checks; no explicit topological pass loops |
| **Per-instance locking** | `DashMap<... Arc<Mutex<NodeInstanceState>>>` — independent instances lock without contention |
| **Cycle-level dirty flag reset** | Dirty flags are cleared at the START of each `update()` call, then inputs that actually changed are re-marked dirty. This prevents stale-dirty infinite propagation |
| **Back-edges to collection gather slots** | Enables collector patterns where a downstream node feeds results back into an earlier-topo-order gather node |
| **Subgraph hierarchy** | Every computation lives in a subgraph. Root = subgraph 0. Fan-out creates child subgraphs. The execution loop is identical at every depth |
| **`TaskQueue` trait** | Decouples execution strategy (sequential now, parallel later) from dirty-trigger logic |

### Key Types

| Type | Role |
|------|------|
| `Engine` | The running incremental engine. Holds topology, workstate, value store, task queue, storage |
| `EngineBuilder` | Fluent builder. Declares nodes, edges, transforms, storage |
| `Topology` | Immutable graph: subgraphs, nodes, slots, edges, topo order |
| `WorkState` | Mutable per-instance flags (present, dirty, error, hash, execution count) |
| `ValueStore` | In-memory value cache with async flush to storage |
| `Transform` trait | User-implemented computation. Declares slot layout via `register()`, executes via `apply()` |
| `TaskQueue` trait | Pluggable scheduler. `enqueue()` accepts `Send` futures, `drain()` runs to completion |

---

## 2. Graph Model

### 2.1 Nodes

Two kinds:

- **I/O node** — a named value endpoint. Declared with `input_node`/`output_node`. Has no transform logic; simply holds a value. An input node has one implicit output slot (slot 0). An output node has one implicit input slot (slot 0).

- **Transform node** — associated with a registered `Transform` instance. Has N input slots and M output slots as declared by `Transform::register`.

### 2.2 Slots

Each slot is either **single** (carries one value) or **collection** (carries a variable-length set of keyed values). Each slot has a `TypeId` for type checking and an optional `KeyExtractor` for collection elements.

```rust
pub(crate) struct SlotKind {
    type_id: TypeId,
    type_name: &'static str,
    is_collection: bool,
    extract_key: Option<fn(&dyn Any) -> u64>,
    deserialize: DeserializeFn,
}
```

### 2.3 Edges

An **edge** connects one output slot to one input slot. Three kinds:

| Edge Kind | Semantics |
|-----------|-----------|
| `Single` | Single value flows directly; same subgraph or cross-scope single |
| `Collection` | Collection flows directly (gather: multiple edges merge into one slot) |
| `SubgraphBoundary` | Collection-output → single-input; creates a child subgraph |

### 2.4 Wiring Constraints

- A **single-value input slot** must receive exactly **one** incoming edge.
- A **collection input slot** (gather) may receive **multiple** incoming edges (implemented as `incoming: HashMap<(NodeId, SlotIndex), Vec<EdgeId>>`). Their elements are merged by key during context building.
- A collection input slot may also be the target of a **back-edge** (an edge from a node later in topological order, creating a cycle). Back-edges are detected during the build phase and are not errors.
- Any node may be the root of at most **one** child subgraph (at most one `SubgraphBoundary` edge incoming as a single-value input).
- Edges between non-I/O nodes must be **type-compatible** (`TypeId` must match at both ends). I/O nodes use a `TypeId::of::<()>()` placeholder and skip type checking.

```
Edge storage:
  outgoing: HashMap<(NodeId, SlotIndex), Vec<EdgeId>>
  incoming: HashMap<(NodeId, SlotIndex), Vec<EdgeId>>
        ↑ Vec<EdgeId> supports multi-edge gather for collection slots
```

### 2.5 Back-Edge Detection

During topological sort, an edge is flagged `is_back_edge = true` if its destination appears **before** its source in the topological order. Back-edges are:

- **Allowed** only into collection input slots
- **Excluded** from the cycle-detection DAG (they do not participate in topo sort)
- **Excluded** from `is_ready` and `is_pending` checks (they don't block or trigger normal re-evaluation)
- **Actuated** separately in `propagate_dirty` — back-edge destinations are **unconditionally enqueued** when the source changes (see [§10](#10-back-edges-and-collector-pattern))

---

## 3. Subgraph Model

### 3.1 Concept

Every computation lives in a **subgraph**. A subgraph is a scope that owns a set of nodes and runs over a set of **instances**.

- The **root subgraph** (id = 0) has exactly one instance (instance key 0 = `UNIT_INSTANCE`).
- **Child subgraphs** have one instance per element of their collection input (the fan-out source).
- Subgraphs form a tree via the `parent: Option<SubgraphId>` field.

### 3.2 Subgraph Rules

**Collection-to-single fan-out** creates the boundary. The source node's collection output slot feeds the child subgraph root node's single input slot. Each element becomes one instance.

**Input wiring allowed from:**

| Source | Constraint |
|--------|------------|
| Same subgraph | Always valid |
| Ancestor subgraph (parent, grandparent, etc.) | Single-value only. Resolved via ancestry chain walk |
| Descendant subgraph (gathered output) | Collection only. Flows up to parent |

**Input wiring prohibited from:**

- Sibling subgraph instances (no cross-instance access)
- Unrelated subgraph scopes

**Subgraph outputs:** an edge from a node inside the subgraph to a node in the parent scope becomes a collection output (one value per instance, gathered by element key).

### 3.3 Instance Ancestry

For cross-scope edge resolution, each instance records its parent:

```rust
pub(crate) struct InstanceAncestry {
    subgraph: SubgraphId,        // child subgraph
    parent_instance: InstanceKey, // instance key in parent that created this one
    element_key: InstanceKey,    // the element key identifying this instance
}
```

When resolving a cross-scope edge from ancestor subgraph A to descendant subgraph B, the engine walks up B's ancestry chain until it reaches A, recovering the correct parent instance key. This supports **multi-level nesting** (grandparent → grandchild).

---

## 4. Build Phase

`EngineBuilder::build` proceeds through these steps:

### Step 1 — Slot Layout Collection
Call `T::register` for each registered transform key. Collect `Vec<SlotKind>` for inputs and outputs.

### Step 2 — Node Classification
Walk declared nodes. Classify as `IoInput`, `IoOutput`, or `Transform(key)`. I/O nodes get implicit slot layouts (one output slot for inputs, one input slot for outputs, both with placeholder `TypeId::of::<()>`).

### Step 3 — Edge Validation and Subgraph Detection
For each edge:
1. Verify source and destination nodes exist.
2. Verify slot indices are in range.
3. Verify `TypeId` compatibility (skipped for I/O-connected edges).
4. If source is collection AND destination is single → **SubgraphBoundary**. Allocate new `SubgraphId`.
5. Validate: at most one boundary edge per destination node.

### Step 4 — Scope Assignment
All nodes start in subgraph 0. BFS from each subgraph root: reachable nodes via non-boundary edges belong to that subgraph. Validate cross-scope rules.

### Step 5 — Topological Sort per Subgraph
Kahn's algorithm on forward edges only (back-edges excluded). Back-edges detected by checking if destination appears before source in topo order. Only collection input slots may be back-edge targets.

### Step 6 — Freeze
Produce immutable `Topology` with:
- `subgraphs: Vec<SubgraphDesc>`
- `nodes: HashMap<NodeId, NodeDesc>`
- `edges: Vec<EdgeDesc>`
- `outgoing: HashMap<(NodeId, SlotIndex), Vec<EdgeId>>`
- `incoming: HashMap<(NodeId, SlotIndex), Vec<EdgeId>>` (multi-edge for gather)

---

## 5. Storage Layer

### 5.1 Key Scheme

Every persisted value is addressed by a composite key:

```
SlotStateKey(subgraph, instance, node, slot) → UUIDv5("nin-slot" + bytes)
ElementKey(slot_key, element_key)            → UUIDv5("nin-elem" + slot_bytes + key_bytes)
CollectionIndexKey(slot_key)                 → UUIDv5("nin-cidx" + slot_bytes)
WorkState snapshot                           → fixed UUID ("nin-wstate")
```

### 5.2 Persisted Record Format

```rust
struct PersistedValue {
    type_name: String,
    bytes: Vec<u8>,     // msgpack of the actual value
    hash: u64,
}

struct PersistedIndex {
    element_keys: Vec<u64>,
}
```

### 5.3 Storage Trait

```rust
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError>;
    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError>;
    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError>;
    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError>;
    async fn checkpoint(&self) -> Result<(), StorageError>;  // default: no-op
    async fn commit(&self) -> Result<(), StorageError>;      // default: no-op
    async fn discard(&self) -> Result<(), StorageError>;     // default: no-op
}
```

`MemoryStorage` is the built-in in-memory backend with proper checkpoint/commit/discard via snapshot-and-replace.

### 5.4 ValueStore

In-memory cache with three DashMaps (single, elements, dirty trackers). On write, sets `dirty_bytes = Some(bytes)`. On commit, flushes all dirty entries. On cache miss for single values, `load_erased` lazily deserializes from storage (used by `Engine::get`).

Collection elements are **not** lazily loaded on warm start (they are recomputed by transform re-execution on the first cycle after warm start).

### 5.5 WorkState Snapshot

The entire `WorkState` (all `SlotState` records and instance key sets) is serialized to msgpack and written under `StorageKey::WORKSTATE` (fixed UUID). Includes element_keys and element_hashes (both are persisted, enabling incremental diff after warm start).

---

## 6. Dirty Flag Protocol

This section describes the **corrected** dirty flag semantics, which differ from the original DESIGN.md.

### 6.1 The Problem

The original design had `mark_present` clear `dirty = false`, then call `propagate_dirty`. Downstream `is_pending` checks source slot's dirty flag — which was now `false`. Result: downstream nodes were **never** enqueued mid-cycle when a transform's output changed.

Only `set_input_value` worked because it explicitly called `mark_dirty` before `propagate_dirty`.

### 6.2 The Fix

Three coordinated changes:

1. **`mark_present` no longer touches `dirty`.** It only sets `present = true`, `error = false`, `hash = H`. The dirty flag is managed exclusively by `mark_dirty` and cycle-level reset.

2. **`commit_outputs` calls `mark_dirty` when value changes.** Before `propagate_dirty`, the source slot is explicitly marked dirty so downstream `is_pending` can detect it.

3. **`reset_cycle` at `update()` start.** At the beginning of each `update()` call, ALL dirty flags and execution counts are cleared. Then only inputs that actually changed get `mark_dirty` through `set_input_value`.

### 6.3 Lifecycle of a Dirty Flag

```
Cold start:
  NodeInstanceState::new() → dirty=true (initial)
  update():
    reset_cycle → dirty=false (cleared)
    set_input_value → mark_dirty(io_node) → dirty=true
    seed_pending_tasks → downstream checked → dirty=true → enqueued
    downstream runs → commit_outputs → mark_dirty → dirty=true
      → propagate_dirty → next downstream sees dirty=true → enqueued
  ...end of cycle...
  (dirty stays true until next cycle)

Subsequent cycle (no input changes):
  update():
    reset_cycle → ALL dirty=false
    no pending inputs → nothing marked dirty
    seed_pending_tasks → nothing pending → nothing runs

Subsequent cycle (input changes):
  update():
    reset_cycle → ALL dirty=false
    set_input_value → mark_dirty(io_node) → dirty=true
    seed_pending_tasks → downstream dirty=true → enqueued
    ...propagation continues as in cold start...
```

### 6.4 Why This Works

- **No stale-dirty infinite loop:** Dirty flags are reset every cycle. Only inputs that actually changed get re-marked. If a transform produces the same output as before, `commit_outputs` detects hash unchanged → no `mark_dirty` → no propagation.
- **No lost propagation:** When output changes, `mark_dirty` is called before `propagate_dirty`, so `is_pending` on the downstream correctly returns `true`.
- **No unnecessary work:** If no inputs changed, `reset_cycle` clears everything and nothing runs.

---

## 7. Update Propagation

### 7.1 Entry Point

```rust
pub async fn update(&self) -> UpdateReport
```

### 7.2 Sequence

```
1. reset_cycle()
   → clear all dirty flags and execution counts

2. Drain pending_inputs (accumulated by set_input())
   For each:
     a. Compute hash of new value.
     b. If hash == stored hash → skip (no-op).
     c. Write to ValueStore.
     d. mark_present(key, 0, hash)  → present=true
     e. mark_dirty(key, 0)          → dirty=true
     f. propagate_dirty(key, 0)

3. seed_pending_tasks()
   → Walk root subgraph topo order.
   → For each node, check is_pending:
     - All non-back-edge inputs must be present+ready
     - At least one source slot must be dirty
   → Enqueue pending nodes.

4. task_queue.drain()
   → Execute tasks until queue is stable-empty.
   → Each execution may enqueue further downstream tasks.

5. Return UpdateReport (errors, cycle limits, counters).
```

### 7.3 `propagate_dirty(src_node, slot)`

Called when an output slot changes. For each outgoing edge:

- **SubgraphBoundary** → `handle_fan_out_change`: diff old vs new element keys, create/remove/retrigger child instances.
- **Single / Collection (forward edge)** → For each destination instance, check `is_pending`. If pending, enqueue.
- **Back-edge** → **Unconditionally** enqueue the destination (the collector must re-run to gather new elements). `is_pending` is NOT checked because back-edges are excluded from `is_pending` checks.

### 7.4 `handle_fan_out_change(child_sg, parent_instance, src_node, src_slot)`

1. Read **old** element hashes from `WorkState` (snapshot from previous evaluation).
2. Read **new** element keys from `WorkState.element_keys` (updated by `commit_outputs`).
3. **Added** keys → `add_instance(child_sg, parent_instance, key)`, enqueue root node.
4. **Removed** keys → `remove_instance(...)`, propagate removal to parent outputs.
5. **Changed** keys → `mark_dirty` on root node's input slot, enqueue root node.

### 7.5 `is_ready(key)` and `is_pending(key)`

```rust
is_ready(key):
  For each input slot:
    Get ALL incoming edges (not just one — multi-edge gather).
    For each non-back-edge:
      Resolve source instance (may be cross-scope via ancestry chain).
      Check source slot is present=true && error=false.
  All sources must be ready → node is ready.

is_pending(key):
  if !is_ready(key) → false.
  For each input slot:
    Get ALL incoming edges.
    For each non-back-edge:
      Check source slot dirty=true.
  Any dirty source → pending.
```

---

## 8. Task Execution

### 8.1 `execute_task(key)`

```rust
1. begin_execute(key):
   - Remove from enqueued set.
   - Set executing=true.
   - Return false if stale (already executing).

2. Re-check is_pending — if no longer pending (inputs settled),
   skip evaluation, go to step 9.

3. Build TransformContext:
   For each input slot:
     - Resolve source via topology.incoming_edges.
     - For single: read one value from ValueStore.
     - For collection gather: read from ALL incoming edges,
       merge elements by key (later sources overwrite earlier).
     - Compute incremental diff (dirty_keys, removed_keys) from
       previous element_hashes snapshot.

4. Call transform.apply(ctx).

5. On error:
   - Mark all output slots error=true.
   - propagate_error (iterative BFS).
   - Record in UpdateReport.

6. On success (commit_outputs):
   For each output slot:
     - Compute hash of new value.
     - If hash changed:
       a. Write to ValueStore.
       b. mark_present(key, slot, hash)
       c. mark_dirty(key, slot)
       d. propagate_dirty(key, slot)
     - If hash unchanged:
       a. mark_present(key, slot, hash) (no dirty, no propagate)

7. Increment execution_count.
   If > cycle_limit → flag as cycle error, mark outputs error.

8. finish_execute(key):
   - Set executing=false.
   - Re-check is_pending — if still pending, re-enqueue.
     (Closes the "dirty signal arrived during execution" window.)
```

### 8.2 Enqueue Deduplication

```rust
try_enqueue(key):
  Lock enqueued set.
  If key in set → return false (already queued).
  If executing=true → return false (finish_execute will re-check).
  Insert key.
  Push task to TaskQueue.
```

This ensures:
- No duplicate evaluations per node per wave
- A collector receiving many element signals collapses into at most one execution
- No dirty signal is lost (finish_execute re-checks)

---

## 9. Scheduling

### 9.1 `TaskQueue` Trait

```rust
pub trait TaskQueue: Send + Sync {
    fn enqueue(&self, fut: SendBoxFuture);
    fn drain(&self) -> SendBoxFuture;  // runs until stable-empty
}
```

`SendBoxFuture = Pin<Box<dyn Future<Output=()> + Send + 'static>>`

The trait is `Send + Sync`, making `Engine` `Send` and compatible with `tokio::spawn`.

### 9.2 `SequentialTaskQueue`

Default implementation. FIFO queue behind `std::sync::Mutex<VecDeque<SendBoxFuture>>`.

```rust
fn drain(&self) {
    loop {
        let task = queue.lock().pop_front();
        match task {
            None => break,   // stable empty
            Some(fut) => fut.await,
        }
    }
}
```

Lock is released before `fut.await`, allowing `enqueue()` from within a running task. No parallelism — tasks run one-at-a-time in order.

---

## 10. Back-Edges and Collector Pattern

### 10.1 Concept

A **back-edge** is an edge from a node B (later in topo order) back to node A (earlier in topo order), feeding into A's **collection gather slot**. This creates a cycle in the graph, but it's not an error because:

- The back-edge is excluded from topological sort (Kahn's algorithm ignores it).
- A's readiness does not depend on B (back-edges are skipped in `is_ready`).
- A's pending status is not triggered by back-edge dirty flags (skipped in `is_pending`).

### 10.2 Lifecycle

```
1. A (the collector) runs and produces an initial empty collection.
2. B receives A's output (forward edge) and processes it.
3. B produces new elements and writes them to a collection output slot.
4. This collection output feeds back into A via the back-edge.
5. propagate_dirty from B detects the back-edge → unconditionally enqueues A.
6. A re-runs, gathers B's new elements (plus its original inputs), produces updated output.
7. If A's output changed → propagate_dirty forward to B.
8. B re-runs with the updated collection from A.
9. Repeat until convergence (A and B both produce unchanged output).
```

### 10.3 Back-Edge Safety

- `execution_count` limits prevent infinite loops within a single `update()` call.
- Cycle limit (default 1000) caps re-executions per node per call.
- Enqueue deduplication ensures A is not enqueued twice in the same wave.

---

## 11. Error Handling and Cycle Detection

### 11.1 Transform Errors

When `Transform::apply` returns `Err`:

1. All output slots of the failed transform are marked `error = true`.
2. BFS propagation: all downstream nodes are also marked error (iterative, not recursive, to avoid non-Send futures).
3. The error is recorded in `UpdateReport::errors` as `(node_uuid, TransformError)`.
4. Propagated nodes are not enqueued (their inputs are now errored).

Errors are **non-sticky** — on the next `update()` call, `reset_cycle` clears error flags. A fixed input value will naturally clear the error.

### 11.2 Cycle Detection

Per-node `execution_count` is incremented each time `execute_task` runs a transform. If it exceeds `cycle_limit` (default 1000, configurable via `EngineBuilder::cycle_limit`):

1. The node's output slots are flagged error (and dirty cleared).
2. The node's UUID is added to `UpdateReport::cycle_limit_exceeded`.
3. `execution_count` is reset at the start of each `update()` call, so the limit applies per-cycle, not across sessions.

---

## 12. Checkpoint / Commit / Discard

Mapped to the `Storage` trait's methods:

### `Engine::checkpoint()`
1. Validate no checkpoint is already active.
2. Call `storage.checkpoint()`.
3. Set `checkpoint_active = true`.

### `Engine::commit()`
1. Validate checkpoint is active.
2. Flush dirty `ValueStore` entries to storage.
3. Serialize `WorkState` snapshot → msgpack → write under `StorageKey::WORKSTATE`.
4. Call `storage.commit()`.
5. Clear `checkpoint_active`.

### `Engine::discard()`
1. Validate checkpoint is active.
2. Call `storage.discard()` — storage rolls back.
3. Evict all `ValueStore` cache entries.
4. Reload `WorkState` from storage (or cold start if no snapshot).
5. Clear `checkpoint_active`.

---

## 13. Warm Start

`EngineBuilder::build` on start:

1. Attempt to load `WorkStateSnapshot` from storage under `StorageKey::WORKSTATE`.
2. Validate compatibility with current `Topology` (same subgraph ids, same node ids, same slot counts). Extra or missing keys are handled gracefully.
3. On success: restore `WorkState` from snapshot. All dirty flags and execution counts from the previous session are preserved. The first `update()` call will `reset_cycle()` and then only process inputs that actually changed.
4. On failure: cold start — `init_cold` creates root I/O node states. First `update()` processes all `set_input()` calls made before `update()`.

Collection element values are **not** cached from the previous session on warm start — they are recomputed when transforms run. Their hashes (`element_hashes`) ARE persisted, so the incremental diff computation works correctly from the first cycle.

---

## Appendix: Corrections to DESIGN.md

The following changes were made during implementation that deviate from the original DESIGN.md:

| Original Design | Actual Implementation | Reason |
|----------------|----------------------|--------|
| `mark_present` sets `dirty=false` | `mark_present` does NOT touch `dirty` | Dirty flag must persist for downstream `is_pending` detection |
| `commit_outputs` marks `present, dirty=false` then calls `propagate_dirty` | `commit_outputs` calls `mark_dirty` before `propagate_dirty` | Otherwise downstream never sees the change |
| `execution_count` persists across cycles | `execution_count` is reset by `reset_cycle` at start of each `update()` | Prevents permanent node lockout after N cycles |
| `incoming` is `(NodeId, SlotIndex) → EdgeId` | `incoming` is `(NodeId, SlotIndex) → Vec<EdgeId>` | Supports multi-edge gather for collection slots |
| `element_hashes` is `#[serde(skip)]` | `element_hashes` is persisted | Enables correct incremental diff after warm start |
| `source_instance_for_edge` falls back to `UNIT_INSTANCE` for deep nesting | Walks the full ancestry chain via `subgraph.parent` | Supports multi-level subgraph nesting |
| Back-edge destinations checked via `is_pending` | Back-edge destinations are **unconditionally enqueued** | `is_pending` skips back-edges; unconditional enqueue is the only way to trigger the collector |
