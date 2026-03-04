# nova-incremental-neo — Design Document

**Version:** 0.5 — pre-implementation  
**Status:** Under review

---

## Table of Contents

1. [Motivation and Goals](#1-motivation-and-goals)
2. [Public API](#2-public-api)
3. [Graph Model](#3-graph-model)
4. [Subgraph Model](#4-subgraph-model)
5. [Architecture Overview](#5-architecture-overview)
6. [Internal Structs and Their Relations](#6-internal-structs-and-their-relations)
7. [Build Phase](#7-build-phase)
8. [Update Algorithm](#8-update-algorithm)
9. [Persistence](#9-persistence)
10. [Error Handling and Cycle Detection](#10-error-handling-and-cycle-detection)
11. [Warm Start](#11-warm-start)
12. [Checkpoint / Commit / Discard](#12-checkpoint--commit--discard)
13. [Module Layout and Dependency Rules](#13-module-layout-and-dependency-rules)
14. [Implementation Style Guidelines](#14-implementation-style-guidelines)

---

## 1. Motivation and Goals

The original `nova-incremental` crate proved the concept but accumulated several design debts:

- Monolithic `ExecutionContext` mixed scheduling concerns with state concerns.
- The internal `Value` type leaked into the public API, making it impossible to evolve independently.
- Fan-out from a collection edge to single-value inputs was handled by repeated graph topology updates, which was fragile and expensive.
- Warm-start was tricky because only values — not dirty flags — were persisted.

`nova-incremental-neo` rebuilds the engine from scratch with these goals:

1. **Clean API boundary** — only `Engine`, `EngineBuilder`, `EngineError`, `UpdateReport`, `Storage`-family, and `Transform`-family are public. All internal machinery is `pub(crate)`.
2. **Unified subgraph model** — every computation lives in a subgraph. The root graph is "subgraph 0". Fan-out from a collection creates a child subgraph rather than duplicating topology nodes.
3. **Edge-driven reactive execution** — writing to an output slot triggers downstream readiness checks immediately; no explicit topological pass loops.
4. **Trait-based task queue** — the scheduler abstraction is a simple `TaskQueue` trait, swappable for parallel execution later.
5. **Edge-centric persistence** — values and dirty flags are keyed by `(SubgraphId, InstanceKey, NodeId, SlotIndex)`, which maps cleanly to the KV storage interface.
6. **Full warm start** — the entire engine state (dirty flags, present flags, hashes, known instance keys) is stored as a single blob, so a warm restart picks up exactly where it left off without any re-evaluation.

---

## 2. Public API

The crate re-exports exactly the following symbols (see `src/lib.rs`):

| Symbol | Kind | Description |
|---|---|---|
| `Engine` | struct | The running incremental engine |
| `EngineBuilder` | struct | Fluent builder for `Engine` |
| `EngineError` | struct | Error from engine construction or operation |
| `UpdateReport` | struct | Summary of one `update()` call |
| `Storage` | trait | Async KV backend (implement for custom storage) |
| `StorageError` | struct | Error from storage operations |
| `StorageKey` | struct | Opaque UUID-based storage key |
| `StorageValue` | struct | Raw bytes wrapper |
| `MemoryStorage` | struct | In-memory `Storage` for tests |
| `Transform` | trait | Implement to define a computation node |
| `TransformContext` | struct | Per-invocation typed I/O |
| `TransformRegisterContext` | trait | Sealed DSL for declaring slot layout |
| `TransformError` | struct | Error from transform execution |
| `IncrementalValue` | trait | Auto-implemented marker for graph-flowable types |
| `KeyExtractor<T>` | trait | Derives a stable `u64` key from a collection element |
| `CollectionInput<T>` | struct | Typed view of a gathered collection input |
| `CollectionChange<T>` | struct | Incremental diff for a collection input |
| `TaskQueue` | trait | Implement to supply a custom task scheduler |
| `SequentialTaskQueue` | struct | Default single-threaded task queue |
| `Uuid` | re-export | From the `uuid` crate |

### 2.1 Transform Authoring

```rust
struct MyTransform;

#[async_trait]
impl Transform for MyTransform {
    fn register(ctx: &mut impl TransformRegisterContext) {
        ctx.input::<SourceFile>();                           // single input, slot 0
        ctx.input_collection::<Token, TokenKey>();          // gather input, slot 1
        ctx.output::<Ast>();                                 // single output, slot 0
        ctx.output_collection::<Diagnostic, DiagKey>();     // spread output, slot 1
    }

    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let src   = ctx.input::<SourceFile>(0)?;
        let toks  = ctx.input_collection::<Token>(1)?;
        let ast   = parse(src, toks.elements)?;
        ctx.output(0, ast)?;
        ctx.output_collection(1, collect_diagnostics(&ast))?;
        Ok(())
    }
}
```

### 2.2 Engine Construction

```rust
let engine = EngineBuilder::new()
    .register("lex",   LexTransform)
    .register("parse", ParseTransform)
    .input_node::<SourceFile>(file_node_id)
    .transform_node(lex_node_id,   "lex")
    .transform_node(parse_node_id, "parse")
    .output_node(ast_node_id)
    .wire_into_slot(file_node_id, lex_node_id, 0)         // file → lex.input[0]
    .wire_slot_to(lex_node_id, 0, ast_node_id)            // lex.output[0] → ast output
    .cycle_limit(500)
    .with_storage(Arc::new(MemoryStorage::new()))
    .build()
    .await?;
```

---

## 3. Graph Model

### 3.1 Nodes

There are two kinds of nodes:

- **I/O node** — a named value endpoint. Declared with `input_node` or `output_node`. Has no transform logic; simply holds a value. Has one implicit output slot (slot 0) for input nodes and one implicit input slot (slot 0) for output nodes.
- **Transform node** — associated with a registered `Transform` instance. Has N input slots and M output slots as declared by `Transform::register`.

### 3.2 Slots and Edges

Each slot is either **single** (carries one value) or **collection** (carries a variable-length set of keyed values).

An **edge** connects one output slot to one input slot. Output slots may fan out to multiple edges (one source, many destinations). Input slots accept at most one incoming edge.

```
      ┌─────────────────┐        ┌─────────────────┐
      │   TransformA    │        │   TransformB    │
      │  out[0]: Single ├──────► │  in[0]: Single  │
      │  out[1]: Single ├──┐     │  in[1]: Single  │
      └─────────────────┘  │     └─────────────────┘
                           │     ┌─────────────────┐
                           └───► │   TransformC    │
                                 │  in[0]: Single  │
                                 └─────────────────┘
```

Fan-out: one output slot → multiple downstream input slots (all receive the same value).

### 3.3 Collection-to-Single Fan-Out (Subgraph Boundary)

When a **collection output slot** is wired to a **single-value input slot**, this is a **subgraph boundary edge**. The destination transform becomes the root of a child subgraph. See Section 4.

### 3.4 Wiring Constraints

- A single-value input slot must receive exactly one incoming edge.
- A collection input slot (gather) may receive multiple incoming edges; their elements are merged and sorted by key. A collection input slot **may also be the target of a back-edge** (an edge from a node that is later in topological order, creating a cycle). Back-edges are how a node can accumulate incrementally growing collections — each new element is published by a downstream node and gathered back. Back-edges are detected during the build phase; they are not an error.
- Any given transform may be the root of at most one child subgraph (i.e., at most one fan-out edge entering it as a single-value input).
- Edges must be type-compatible (`TypeId` of the source slot's element type must equal the destination slot's element type).

---


## 3b. Task Queue

The `TaskQueue` controls how the engine schedules node execution during an
`update()` call.

### `TaskQueue` Trait

```rust
pub trait TaskQueue: Send + Sync {
    /// Enqueue a `Send + 'static` future for execution.
    /// May be called from within a running task.
    fn enqueue(&self, fut: Pin<Box<dyn Future<Output = ()> + Send + 'static>>);

    /// Run all enqueued tasks until the queue reaches a stable empty state
    /// (queue empty AND no task currently executing).
    fn drain(&self) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
}
```

`TaskQueue: Send + Sync` ensures that [`Engine`] is `Send`, making it compatible
with `tokio::spawn`.

Task futures passed to `enqueue` must be `Send + 'static`. In the engine
internals, `execute_task` satisfies this because:
- All `RunContext` fields (`Topology`, `WorkState`, `ValueStore`) are `Arc<T>`
  where `T: Send + Sync`.
- `tokio::sync::MutexGuard` (used in `WorkState`) is `Send`.
- `std::sync::Mutex` guards are never held across `.await` points.
- The enqueue deduplication set (`WorkState::enqueued`) uses `std::sync::Mutex`,
  making `try_enqueue_task` a **sync function** — breaking the async call cycle
  that would otherwise prevent `execute_task` from being `Send`.

### `SequentialTaskQueue`

The built-in default. Runs tasks one at a time in FIFO order using an
`Arc<std::sync::Mutex<VecDeque<SendBoxFuture>>>`.

Supply via the builder:
```rust
// Use default:
EngineBuilder::new()
    .with_storage(storage)
    .build()
    .await?;

// Supply a custom queue:
EngineBuilder::new()
    .with_storage(storage)
    .with_task_queue(MyParallelQueue::new())
    .build()
    .await?;
```

## 4. Subgraph Model

### 4.1 Concept

Every computation lives in a **subgraph**. A subgraph is a scope that owns a set of nodes and runs over a set of **instances**. The root subgraph (id = 0) has exactly one instance (the "unit" instance). Child subgraphs have one instance per element of their collection input.

This unifies the handling of the root graph and fan-out templates: the execution loop is identical at every depth.

### 4.2 Visual

```
Root Subgraph (id=0, 1 instance: unit)
│
│  [InputNode: file_list]
│       │  (collection out → single in = fan-out boundary)
│       ▼
│  ┌─────────────────────────────────────────────────┐
│  │  Child Subgraph (id=1, N instances: one per file) │
│  │                                                   │
│  │  [TransformNode: lex]                             │
│  │       │  (single out)                             │
│  │  [TransformNode: parse]                           │
│  │       │  (single out → parent = gathered output)  │
│  └───────┼───────────────────────────────────────────┘
│          │ (collection in the parent scope)
│  [OutputNode: ast_list]
```

### 4.3 Subgraph Rules

**Exactly one collection-to-single fan-out edge per subgraph root.** This is the single collection input that defines the set of instances. To combine two collections, first zip them using a transform in the parent scope, then fan-out the combined collection.

**Input wiring allowed from:**
- Any node in the same subgraph.
- A gathered (collection) output of any descendant subgraph — this is already a collection in the current scope.
- Any node in any ancestor subgraph (parent, grandparent, etc.) — single-value only; if the ancestor value changes, all instances are re-evaluated incrementally.

**Input wiring prohibited from:**
- Sibling subgraph instances (there is no cross-instance access).
- Unrelated subgraph scopes.

**Subgraph outputs** — any edge from a node inside the subgraph that connects to a node in the parent scope becomes a collection output: one value per instance, gathered by element key.

**Empty collection input** — the subgraph produces zero instances and all its parent-facing collection outputs are empty.

---

## 5. Architecture Overview

```
┌───────────────────────────────────────────────────────────────────┐
│  Public API                                                         │
│  Engine  EngineBuilder  UpdateReport  Transform  TransformContext  │
└───────────────────────────────┬───────────────────────────────────┘
                                │ owns
        ┌───────────────────────▼────────────────────────┐
        │                  Engine (internal)               │
        │  topology: Arc<Topology>                         │
        │  workstate: Arc<RwLock<WorkState>>               │
        │  value_store: Arc<ValueStore>                    │
        │  task_queue: Arc<dyn TaskQueue>                  │
        │  storage: Arc<dyn Storage>                       │
        └──────┬────────────────┬────────────────┬────────┘
               │                │                │
     ┌─────────▼──┐   ┌─────────▼──┐   ┌────────▼───────┐
     │  Topology   │   │  WorkState │   │  ValueStore    │
     │  (immutable)│   │  (mutable) │   │  (mutable)     │
     └────────────┘   └────────────┘   └────────────────┘
```

### Responsibilities

| Component | Responsibility |
|---|---|
| `Topology` | Immutable graph structure: subgraphs, nodes, slots, edges, topo order |
| `WorkState` | Per-(subgraph, instance, node, slot): present/dirty/error/hash flags |
| `ValueStore` | In-memory cache of current values; lazy-loaded from `Storage` |
| `TaskQueue` | Trait for enqueueing async tasks; sequential impl by default |
| `Storage` | Trait for persistent KV backend (`MemoryStorage` built-in) |

---

## 6. Internal Structs and Their Relations

### 6.1 `SubgraphId`, `InstanceKey`, `NodeId`, `SlotIndex`

```rust
pub(crate) struct SubgraphId(u32);  // 0 = root
pub(crate) struct InstanceKey(u64); // 0 = unit (root subgraph)
pub(crate) struct NodeId(Uuid);
pub(crate) type   SlotIndex = usize;
```

`SubgraphId` is assigned sequentially during the build phase. `InstanceKey` is the element key from `KeyExtractor`; for the root subgraph it is the constant 0.

### 6.2 `Topology`

```rust
pub(crate) struct Topology {
    subgraphs: Vec<SubgraphDesc>,       // indexed by SubgraphId
    nodes: HashMap<NodeId, NodeDesc>,
    edges: Vec<EdgeDesc>,               // all edges, flat
    // Pre-computed: which edges leave each output slot
    outgoing: HashMap<(NodeId, SlotIndex), Vec<EdgeId>>,
    // Pre-computed: which edge feeds each input slot
    incoming: HashMap<(NodeId, SlotIndex), EdgeId>,
}

pub(crate) struct SubgraphDesc {
    id: SubgraphId,
    parent: Option<SubgraphId>,
    /// None for root subgraph (unit instance).
    collection_input_edge: Option<EdgeId>,
    /// Nodes in topological order.
    topo_order: Vec<NodeId>,
    io_input_nodes: Vec<NodeId>,
    io_output_nodes: Vec<NodeId>,
    child_subgraphs: Vec<SubgraphId>,
}

pub(crate) struct NodeDesc {
    id: NodeId,
    kind: NodeKind,
    subgraph: SubgraphId,
    input_slots: Vec<SlotKind>,    // from Transform::register
    output_slots: Vec<SlotKind>,
}

pub(crate) enum NodeKind {
    IoInput,
    IoOutput,
    Transform { transform: Arc<dyn ErasedTransform> },
}

pub(crate) struct EdgeDesc {
    id: EdgeId,
    from_node: NodeId,
    from_slot: SlotIndex,
    to_node: NodeId,
    to_slot: SlotIndex,
    kind: EdgeKind,
}

pub(crate) enum EdgeKind {
    /// Single value flows directly.
    Single,
    /// Collection flows directly (gather: multiple edges merge into one slot).
    Collection,
    /// Fan-out: collection-out slot → single-in slot; defines a subgraph boundary.
    /// The child subgraph id is stored here.
    SubgraphBoundary { child: SubgraphId },
}
```

`Topology` is produced once by `EngineBuilder::build` and never mutated. All mutable state lives in `WorkState` and `ValueStore`.

### 6.3 `WorkState`

`WorkState` is the mutable heart of the engine. Rather than one giant lock over all state, it is structured so that per-node-instance data can be locked individually, reducing contention when many instances are being evaluated in parallel.

#### Top-level structure

```rust
pub(crate) struct WorkState {
    /// Per-(SubgraphId, InstanceKey, NodeId): fine-grained instance state.
    /// Each entry is an independently lockable unit.
    node_states: DashMap<NodeInstanceKey, Arc<Mutex<NodeInstanceState>>>,

    /// Per-(SubgraphId, InstanceKey): which child InstanceKeys are known.
    /// Locked per (SubgraphId, InstanceKey) parent scope.
    instances: DashMap<SubgraphInstanceKey, Arc<Mutex<Vec<InstanceKey>>>>,

    /// Global set of currently-enqueued tasks.
    /// Guarded separately because enqueue checks must be atomic.
    enqueued: Mutex<HashSet<NodeInstanceKey>>,
}

pub(crate) struct NodeInstanceKey {
    subgraph: SubgraphId,
    instance: InstanceKey,
    node:     NodeId,
}

pub(crate) struct SubgraphInstanceKey {
    subgraph: SubgraphId,
    instance: InstanceKey, // parent instance key
}
```

#### `NodeInstanceState`

All mutable flags and hashes for one `(subgraph, instance, node)` triple live in one struct, independently lockable via its own `Mutex`:

```rust
pub(crate) struct NodeInstanceState {
    /// Per output slot.
    output_slots: Vec<SlotState>,
    /// Number of times this node has been *executed* (not enqueued) in the
    /// current update cycle. Used for cycle detection.
    execution_count: u32,
    /// True while a task for this node instance is currently *executing*.
    /// Set to true when the task begins; cleared when it finishes.
    /// New enqueue requests are rejected while this is true (see Section 8.7).
    executing: bool,
}

pub(crate) struct SlotState {
    present: bool,
    dirty: bool,
    error: bool,
    hash: Option<ValueHash>,
    /// For collection output slots: sorted list of known element keys.
    element_keys: Vec<u64>,
    /// For collection output slots: snapshot of element hashes at last evaluation,
    /// used to compute the CollectionChange diff for downstream gather inputs.
    element_hashes: HashMap<u64, ValueHash>,
}
```

#### Key operations on WorkState

- `get_or_create_node_state(key) -> Arc<Mutex<NodeInstanceState>>` — creates with default (absent, dirty, no error) on first access.
- `is_ready(topology, subgraph, instance, node) -> bool` — for each input slot, resolves the source output slot via topology, locks its `NodeInstanceState`, checks `present=true && error=false`. This requires locking multiple `NodeInstanceState` entries sequentially; the order is fixed by topo index to avoid deadlocks.
- `is_pending(topology, subgraph, instance, node) -> bool` — `is_ready` AND at least one source slot has `dirty=true`.
- `try_enqueue(key) -> bool` — atomically checks the `enqueued` set and inserts; returns `true` if successfully enqueued, `false` if already present.
- `begin_execute(key) -> bool` — atomically: removes key from `enqueued`, locks the node state, sets `executing=true`. Returns `false` if the node state has already been reset (stale task).
- `finish_execute(key)` — locks the node state, clears `executing=false`, increments `execution_count`.
- `add_instance(subgraph, parent_instance, element_key)` — inserts into the `instances` map.
- `remove_instance(subgraph, parent_instance, element_key)` — removes from `instances` map and removes all `NodeInstanceState` entries for that instance.

### 6.4 `ValueStore`

```rust
pub(crate) struct ValueStore {
    cache: DashMap<SlotStateKey, Arc<dyn Any + Send + Sync>>,
    // element-level cache for collection slots
    element_cache: DashMap<ElementKey, Arc<dyn Any + Send + Sync>>,
}

pub(crate) struct ElementKey {
    slot: SlotStateKey,
    element_key: u64,
}
```

`ValueStore` holds the actual deserialized values in memory. On a cache miss, values are loaded from `Storage` and deserialized lazily. This avoids holding the full value graph in memory if not needed.

#### Key operations

- `get<T>(key)` → `Option<T>` — returns cached value or loads from storage.
- `set<T>(key, value)` — stores in cache (does not write to storage yet).
- `get_collection<T>(slot_key)` → `Vec<(u64, T)>` — returns all elements of a collection slot.
- `set_element<T>(element_key, value)` — stores one collection element in cache.
- `flush(storage)` — serialize and write all dirty cache entries to storage.
- `evict(key)` — remove from cache (forces reload from storage next access).


### 6.5 `ErasedTransform` (internal)

```rust
pub(crate) trait ErasedTransform: Send + Sync {
    fn slot_inputs(&self) -> &[SlotKind];
    fn slot_outputs(&self) -> &[SlotKind];
    fn apply_erased<'a>(
        &'a self,
        ctx: &'a mut TransformContext,
    ) -> BoxFuture<'a, Result<(), TransformError>>;
}
```

`ErasedTransform` is a `dyn`-safe wrapper over the generic `Transform` trait. Produced during `EngineBuilder::build` and stored in `NodeDesc::kind`.

### 6.6 `TransformContext` (public, internal fields)

```rust
pub struct TransformContext {
    pub(crate) inputs: Vec<ContextInput>,
    pub(crate) outputs: Vec<ContextOutput>,
    pub(crate) slot_kinds_in: Vec<SlotKind>,
    pub(crate) slot_kinds_out: Vec<SlotKind>,
}

pub(crate) enum ContextInput {
    Single(Arc<dyn Any + Send + Sync>),
    Collection {
        elements: Vec<Arc<dyn Any + Send + Sync>>,
        keys: Vec<u64>,
        diff: ErasedCollectionChange,
    },
    Absent,
}

pub(crate) enum ContextOutput {
    Single(Arc<dyn Any + Send + Sync>, ValueHash),
    Collection(Vec<(u64, Arc<dyn Any + Send + Sync>, ValueHash)>),
    Absent,
}
```

The engine fills `inputs` before calling `Transform::apply`. After `apply`, it reads `outputs` to determine what changed.

---

## 7. Build Phase

`EngineBuilder::build` proceeds in these steps:

### Step 1 — Slot Layout Collection
For each registered transform key, call `T::register` with a `SlotRegistrar` to collect `SlotKind` vectors.

### Step 2 — Node Classification
Walk declared nodes. Classify each as `IoInput`, `IoOutput`, or `Transform(key)`. Build a `NodeId → NodeDesc` map (slots not yet assigned).

### Step 3 — Edge Validation and Subgraph Detection
For each declared edge:
1. Verify source and destination nodes exist.
2. Verify slot indices are in range.
3. Verify `TypeId` compatibility at both ends.
4. If the source slot is `is_collection=true` AND the destination slot is `is_collection=false` → this is a **SubgraphBoundary edge**. Allocate a new `SubgraphId`. Record the destination node as the subgraph root.
5. Validate: each node may be the root of at most one child subgraph (at most one `SubgraphBoundary` edge incoming to it as a single-value input).

### Step 4 — Scope Assignment
Start all nodes in subgraph 0. BFS from each subgraph root: nodes reachable via non-boundary edges belong to that subgraph. A node may belong to only one subgraph.

Validate scope rules for each edge:
- An edge within the same subgraph: always valid.
- An edge from a parent (or ancestor) subgraph to the current subgraph: valid only if the destination is a single-value slot.
- An edge from a child subgraph's gathered output to the current subgraph: valid (this is how subgraph results flow back up).
- Any other cross-scope edge: error.

### Step 5 — Topological Sort per Subgraph
Within each subgraph, perform Kahn's algorithm on the node DAG considering only **forward edges** (non-back-edges). Back-edges (to collection input slots) are recorded separately in `SubgraphDesc::back_edges` and are excluded from the cycle-detection DAG. Cross-scope edges are treated as if the source is an external dependency (already available). If a cycle among forward edges is detected within a scope, return a build error — such a cycle would mean a single-value node depends on itself, which has no valid evaluation order.

### Step 6 — Freeze into Topology
Produce immutable `Topology`, `NodeDesc` map, `EdgeDesc` list, `outgoing` and `incoming` index maps.

---

## 8. Update Algorithm

### 8.1 Entry Point

```rust
pub async fn update(&self) -> UpdateReport
```

Runs until the task queue is empty or the cycle limit is exceeded.

### 8.2 Initial Dirty Propagation

When `set_input(node_id, value)` is called:
1. Compute hash of new value.
2. If hash == stored hash in `WorkState` → no-op (value unchanged).
3. Otherwise: write value to `ValueStore`, update `WorkState` (mark present, dirty, new hash).
4. Call `propagate_dirty(subgraph_0, UNIT_INSTANCE_KEY, node_id, slot=0)`.

`propagate_dirty(subgraph, instance, node, slot)`:
1. For each edge in `topology.outgoing[(node, slot)]`:
   - Get destination `(dst_node, dst_slot)`.
   - Update `WorkState` for the source slot: `dirty=true` (already done at call site for initial write).
   - Check if `dst_node` is a subgraph root connected via a `SubgraphBoundary` edge:
     - Yes: run `propagate_collection_change(child_subgraph, instance, new_elements, old_elements)`.
   - Otherwise: if `is_pending(dst_node, instance)` → enqueue `dst_node` **only if it is not already enqueued** (see Section 8.7).

### 8.3 Collection Change Propagation

When a collection output slot changes (element added, removed, or changed):

```
propagate_collection_change(child_subgraph, parent_instance, added_keys, removed_keys, changed_keys)
```

- **Added element key K:**
  - `workstate.add_instance(child_subgraph, parent_instance, K)`.
  - Mark the collection-input edge's state as present+dirty for instance K.
  - Enqueue the child subgraph root node for instance K **if not already enqueued**.

- **Removed element key K:**
  - `workstate.remove_instance(child_subgraph, parent_instance, K)`.
  - Remove all cached values for that instance from `ValueStore`.
  - Mark all parent-side collection output edges for this subgraph as dirty (element removed).
  - Enqueue any downstream nodes in the parent that receive these collection outputs and are now pending, **if not already enqueued**.

- **Changed element key K:**
  - Update value in `ValueStore` for the collection-input slot, instance K.
  - Mark collection-input edge dirty for instance K.
  - Enqueue child subgraph root for instance K **if not already enqueued**.

**Back-edge / collector re-evaluation:**  
When a node publishes a new element into a collection slot that feeds back (via a back-edge) into a gather slot on an earlier-topo-order node (the "collector"), the collector node is enqueued for re-evaluation. Because the collector may already be in the queue from a previous change in the same update cycle, the enqueue is idempotent — it must **not** add a second copy. The enqueued flag in `WorkState` (see Section 8.7) enforces this. The collector will see the full updated collection when it eventually runs.

### 8.4 Task Execution

Each enqueued task captures `(subgraph_id, instance_key, node_id)` in its closure:

```
execute_task(subgraph, instance, node):
  1. Call workstate.begin_execute(key):
     - Lock the NodeInstanceState for this key.
     - Verify executing=false (safety guard).
     - Set executing=true.
     - Remove the key from the enqueued set.
     If begin_execute returns false (state was reset under us), abort.
  2. Re-check is_pending — if no longer pending (inputs became clean between
     enqueue and execution start), skip evaluation and go to step 9.
  3. Build TransformContext:
     - For each input slot of this node:
       a. Look up the source output slot via topology.incoming.
       b. Lock the source NodeInstanceState (use a fixed ordering by
          NodeInstanceKey to avoid deadlock when locking multiple states).
       c. Read slot value from ValueStore.
       d. If single slot: ContextInput::Single(value).
       e. If collection/gather: collect all elements from ValueStore, read
          element_hashes snapshot from SlotState, compute CollectionChange diff.
  4. Release all source NodeInstanceState locks before calling apply.
  5. Call transform.apply(ctx).
  6. If error:
     - Lock own NodeInstanceState; mark all output slots error=true.
     - Propagate error downstream (propagate_error).
     - Record error in the shared UpdateReport accumulator.
  7. If ok:
     - For each output slot with a written value:
       a. Compute hash of new value.
       b. Lock own NodeInstanceState.
       c. If hash != old hash (or was absent):
          - Write to ValueStore.
          - Update SlotState: present=true, dirty=false, hash=new_hash,
            update element_hashes snapshot for collections.
          - Release lock.
          - Call propagate_dirty(subgraph, instance, node, slot).
       d. Else: update SlotState: present=true, dirty=false. Release lock.
  8. Increment execution_count for this (subgraph, instance, node).
     If execution_count > cycle_limit → record cycle error (Section 10).
  9. Call workstate.finish_execute(key):
     - Lock NodeInstanceState; set executing=false.
     - Re-check: if any input became dirty while we were executing (a race
       with a concurrent propagate_dirty), call try_enqueue(key) again.
       This closes the "dirty signal lost during execution" window.
```

### 8.5 Cycle Limit

A per-`(subgraph, instance, node)` **execution counter** (`execution_count` in `NodeInstanceState`) is incremented each time `execute_task` actually runs. If it exceeds `cycle_limit`:
- The node is not enqueued again.
- `UpdateReport::cycle_limit_exceeded` records the node's UUID.
- The node's output slots are flagged with error.
- Downstream propagation continues with the error flag.

Using an execution counter (not an enqueue counter) prevents false positives: a collector node that is enqueued once but receives many back-edge element arrivals in one update cycle should not be penalised for each arrival.

### 8.6 Convergence

`Engine::update` calls `task_queue.drain()` once. `drain()` returns only when the queue is empty and no task is executing, meaning all in-flight work has completed and no further enqueues can arrive from running tasks. Because:
- Each dirty signal adds at most one task per destination node (deduplication prevents duplicates).
- Each execution either produces no change (stops propagating) or triggers at most one new task per downstream node.
- The cycle limit bounds infinite loops.

After `drain()` returns, the engine is in a stable state: every pending computation has been executed, and no node is still dirty.

### 8.7 Enqueue Deduplication and Parallel Execution Safety

The `WorkState` maintains:
1. `enqueued: Mutex<HashSet<NodeInstanceKey>>` — tasks queued but not yet executing.
2. `executing: bool` inside each `NodeInstanceState` — task currently running.

**try_enqueue(key):**
```
Lock enqueued set.
If key in enqueued → return false.
Lock NodeInstanceState for key.
If executing=true → return false (finish_execute will re-check after it finishes).
Insert key into enqueued set. Release both locks.
Push task closure to TaskQueue.
Return true.
```

**begin_execute(key):**
```
Lock enqueued set. Remove key.
Lock NodeInstanceState. Assert executing=false. Set executing=true.
Release both locks. Return true.
```

**finish_execute(key):**
```
Lock NodeInstanceState. Set executing=false.
If is_pending(key) → call try_enqueue(key).
Release lock.
```

This protocol ensures:
- A node is never evaluated twice simultaneously.
- No dirty signal is lost even if it arrives while the node is running.
- A collector receiving many element signals collapses them into at most one execution per wave of changes.

---

## 9. Persistence

### 9.1 Addressing Scheme

Every persisted value is identified by a `SlotStateKey`:

```rust
pub(crate) struct SlotStateKey {
    subgraph:  SubgraphId,
    instance:  InstanceKey,
    node:      NodeId,
    slot:      SlotIndex,
}
```

This maps to a `StorageKey` (a UUID) via a deterministic hash (e.g., UUIDv5 over the msgpack serialization of the key tuple).

For collection output slots, individual elements have an `ElementKey`:

```rust
pub(crate) struct ElementKey {
    slot_key:    SlotStateKey,
    element_key: u64,
}
```

Also mapped to a UUID via UUIDv5.

A special **collection index key** stores the list of known element keys for a collection slot (also addressed as a `SlotStateKey` variant).

### 9.2 Stored Records

**Single-value slot:**
```
StorageKey::from(slot_key) → msgpack {
    type_key: String,       // stable type name / discriminant
    value_bytes: Vec<u8>,   // msgpack of the actual value
    hash: u64,
}
```

**Collection element:**
```
StorageKey::from(element_key) → msgpack {
    type_key: String,
    value_bytes: Vec<u8>,
    hash: u64,
}
```

**Collection index:**
```
StorageKey::collection_index(slot_key) → msgpack Vec<u64>  // sorted element keys
```

### 9.3 WorkState Snapshot

The entire `WorkState` (all `SlotState` records, all instance key sets) is serialized to msgpack and written under a single well-known `StorageKey::WORKSTATE`. This is the only key whose UUID is hard-coded; all others are derived.

```rust
#[derive(Serialize, Deserialize)]
struct WorkStateSnapshot {
    version: u32,
    states: Vec<(SlotStateKeyRaw, SlotStateRecord)>,
    instances: Vec<(SubgraphInstanceKeyRaw, Vec<u64>)>,
}
```

---

## 10. Error Handling and Cycle Detection

### Transform Errors

`TransformError` is returned from `Transform::apply`. On error:
- All output slots of the failed transform are marked `error=true` in `WorkState`.
- Error propagates downstream: for each downstream node whose readiness is now violated (due to an errored input), it is also marked blocked and not enqueued.
- The error is recorded in `UpdateReport::errors` as `(node_uuid, TransformError)`.

Errors are **not** sticky across `update()` calls. On the next `set_input()` followed by `update()`, dirty propagation re-evaluates from scratch (clearing error flags on re-evaluation attempt).

### Cycle Detection

Per-`(subgraph, instance, node)` enqueue counters are maintained in `WorkState`. If the counter exceeds `cycle_limit` (default 1000, configurable):
- The node is not enqueued again.
- `UpdateReport::cycle_limit_exceeded` records the node's UUID.
- The node's output slots are flagged with error.
- Downstream propagation continues with the error flag.

The global limit means deeply nested subgraphs with many instances are all subject to the same total budget.

---

## 11. Warm Start

On `EngineBuilder::build`:
1. Attempt to load `WorkStateSnapshot` from storage under `StorageKey::WORKSTATE`.
2. Validate that the snapshot's subgraph/node structure is compatible with the current `Topology`. Compatibility means same `SubgraphId`s, same `NodeId`s, same slot counts. Extra or missing keys are handled gracefully (new nodes start dirty; removed nodes are ignored).
3. If valid: restore `WorkState` from snapshot. Dirty nodes are enqueued immediately when `update()` is first called. Clean nodes with present values are left as-is (values loaded lazily from storage).
4. If absent or incompatible: cold start — all nodes marked dirty, `ValueStore` empty.

Warm start means that if the compiler restarts between incremental builds, it picks up from the previous state without re-running any transform whose inputs haven't changed.

---

## 12. Checkpoint / Commit / Discard

These map cleanly to the `Storage` trait's checkpoint methods.

### `Engine::checkpoint()`
1. Validate no checkpoint is already active.
2. Call `storage.checkpoint()`.
3. Set a `checkpoint_active` flag.

### `Engine::commit()`
1. Validate checkpoint is active.
2. Flush all dirty `ValueStore` entries to storage: call `value_store.flush(storage)`.
3. Serialize `WorkState` → msgpack → write to `StorageKey::WORKSTATE`.
4. Call `storage.commit()`.
5. Clear `checkpoint_active`.

### `Engine::discard()`
1. Validate checkpoint is active.
2. Call `storage.discard()` — storage rolls back to pre-checkpoint state.
3. Evict all `ValueStore` cache entries (they may reference rolled-back values).
4. Reload `WorkStateSnapshot` from storage (which now reflects the pre-checkpoint state).
5. Clear `checkpoint_active`.

---

## 13. Module Layout and Dependency Rules

### 13.1 File Map

```
nova-incremental-neo/src/
    lib.rs           ← Public re-exports only. No logic.
    report.rs        ← UpdateReport struct only. No deps on other internal modules.
    storage.rs       ← Storage trait, StorageError, StorageKey, StorageValue,
                       MemoryStorage. No deps on other internal modules.
    transform.rs     ← Transform trait, TransformContext, TransformRegisterContext,
                       TransformError, IncrementalValue, KeyExtractor, CollectionInput,
                       CollectionChange, SlotKind, SlotRegistrar (pub(crate)).
                       Depends on: nothing internal.
    keys.rs          ← SlotStateKey, ElementKey, and their → StorageKey derivation.
                       Depends on: storage.rs (StorageKey only).
    topology.rs      ← Topology, SubgraphDesc, NodeDesc, EdgeDesc, SubgraphId,
                       InstanceKey, NodeId, EdgeId, EdgeKind.
                       Depends on: transform.rs (SlotKind, ErasedTransform),
                                   keys.rs (SlotStateKey).
    workstate.rs     ← WorkState, SlotState, enqueue deduplication set.
                       Depends on: topology.rs (SubgraphId, InstanceKey, NodeId,
                                   SlotIndex, Topology for readiness queries),
                                   keys.rs (SlotStateKey).
    value_store.rs   ← ValueStore, in-memory cache + lazy load from Storage.
                       Depends on: storage.rs (Storage, StorageKey, StorageValue),
                                   keys.rs (SlotStateKey, ElementKey),
                                   transform.rs (IncrementalValue, TransformError).
    task_queue.rs    ← TaskQueue trait, SequentialTaskQueue.
                       Depends on: nothing internal.
    runner.rs        ← execute_task, propagate_dirty, propagate_error,
                       propagate_collection_change, build_transform_context.
                       Depends on: topology.rs, workstate.rs, value_store.rs,
                                   task_queue.rs, transform.rs, report.rs.
    engine.rs        ← Engine, EngineBuilder, EngineError.
                       Depends on: all of the above (assembles them).
```

### 13.2 Dependency Graph (acyclic)

```
lib.rs
  └─ engine.rs
       ├─ runner.rs
       │    ├─ topology.rs
       │    │    ├─ transform.rs (SlotKind, ErasedTransform)
       │    │    └─ keys.rs
       │    │         └─ storage.rs (StorageKey)
       │    ├─ workstate.rs
       │    │    ├─ topology.rs
       │    │    └─ keys.rs
       │    ├─ value_store.rs
       │    │    ├─ storage.rs
       │    │    ├─ keys.rs
       │    │    └─ transform.rs (IncrementalValue)
       │    ├─ task_queue.rs   (no internal deps)
       │    ├─ transform.rs    (TransformContext)
       │    └─ report.rs       (no internal deps)
       └─ storage.rs
```

**Rule:** No module may import from a module that is higher in this tree. Cyclic `use` paths are a build error. `runner.rs` is the only module that depends on the majority of internals — all coordination logic is concentrated there to keep the other modules simple and independently testable.

### 13.3 File Size Budget

Each file should stay under **600 lines** of Rust code. If a file exceeds this, extract a sub-module (e.g., `topology/builder.rs` and `topology/query.rs` under `topology/mod.rs`).

---

## 14. Implementation Style Guidelines

### 14.1 Naming Conventions

| Category | Convention | Example |
|---|---|---|
| Public types | `PascalCase` | `TransformContext`, `EngineBuilder` |
| Internal types | `PascalCase` with `pub(crate)` | `SlotStateKey`, `SubgraphDesc` |
| Functions | `snake_case`, verb-first for actions | `propagate_dirty`, `build_context`, `mark_error` |
| Query functions | `snake_case`, adjective/noun or `is_`/`has_` | `is_pending`, `is_ready`, `has_error` |
| Constructors | `new()` or `from_*(...)` | `WorkState::new()`, `SlotStateKey::from_parts(...)` |
| Constants | `SCREAMING_SNAKE_CASE` | `UNIT_INSTANCE_KEY`, `WORKSTATE_STORAGE_KEY` |
| Type aliases | `PascalCase` | `SlotIndex = usize`, `ValueHash = u64` |

### 14.2 Function Length and Complexity

- Functions should do **one thing** and fit within ~40 lines of code.
- If a function has more than 3 levels of nesting, extract inner blocks into named helpers.
- Prefer returning early (`?`, `return`) over deep `if/else` nesting.
- Each helper function must have a doc comment explaining what it does (not how).

### 14.3 Error Handling

- Use `Result<T, E>` for all fallible operations. Never `panic!` in library code except for invariant violations that indicate a programming error (use `unreachable!("reason")` with a clear message).
- Propagate errors with `?`; only map errors at boundary points (e.g., converting `StorageError` into `EngineError`).
- `TransformError` is the only error type that crosses the public API from the user's transform code into the engine.

### 14.4 Concurrency and Locking

- `WorkState` is protected by a `parking_lot::RwLock`. The write lock must never be held across an `.await` point.
- `ValueStore` uses `DashMap` for fine-grained concurrent access; individual entry locks are held as briefly as possible.
- `TaskQueue::enqueue` must be callable from any context without holding any engine lock.
- Prefer `Arc<T>` for shared ownership over raw references when lifetime complexity arises.

### 14.5 Testing

- Each internal module must have a `#[cfg(test)]` section at the bottom of its file covering its own logic in isolation.
- `engine.rs` integration tests live in a separate `tests/` directory at the crate root.
- Tests must not rely on `tokio::test` with real I/O; use `MemoryStorage` for all persistence tests.
- Test function names follow: `test_<what>_<condition>_<expected_outcome>`.

### 14.6 Documentation

- All `pub` and `pub(crate)` types and functions must have a doc comment.
- Doc comments describe **what** the item does and **why** it exists, not implementation details.
- Implementation notes (the "how") go in `//` inline comments directly above the relevant code.
- Use `# Examples` sections in doc comments for public API items.

### 14.7 No Internal State in Public Types

- `TransformContext`'s `pub(crate)` fields must never be accessed from user code. The public methods (`input`, `output`, etc.) are the only interface.
- `StorageKey` and `StorageValue` expose `as_uuid` / `as_bytes` for storage implementors; they do not expose internal structure for the engine's own use (the engine uses `keys.rs` helpers instead).

### 14.8 Avoid Premature Abstraction

- Do not add trait bounds, generics, or abstractions that are not needed for the current implementation. Comment with `// TODO(parallel): replace with pool-based TaskQueue` where future extension points exist.
- Prefer concrete types with clear ownership over heavily-generic code where there is only one likely implementor.

---

## Appendix — Key Design Decisions and Rationale

| Decision | Rationale |
|---|---|
| Unified subgraph model (root = subgraph 0) | Eliminates special-casing at every level; execution loop is identical for root and children |
| Back-edges allowed to collection input slots | Enables collector patterns where a downstream node feeds results back into a gather node; back-edges are excluded from topo sort but handled in the update loop |
| Edge-driven reactive triggers | Avoids repeated full-graph topo passes; each change ripples exactly as far as needed |
| Enqueue deduplication via `HashSet` in WorkState | A collector receiving many element arrivals in one update cycle is enqueued only once; it sees the full updated collection when it runs |
| Cycle limit on execution count, not enqueue count | Avoids false positives from collector patterns where many enqueues correctly collapse into few executions |
| Output-slot-centric persistence (not edge-centric) | Fan-out means one output slot → many edges; storing per-slot avoids redundant writes |
| WorkState persisted as a single blob | Atomic: either the whole state is valid or we do a cold start; no partial corruption |
| `TaskQueue` trait | Decouples execution strategy (sequential now, parallel later) from trigger logic |
| Errors are non-sticky (cleared on re-evaluation) | Avoids permanent poisoning; a fixed input clears the error naturally |
| `KeyExtractor` for collection element identity | Stable, pure key derivation enables correct diffing across sessions |
| `NodeInstanceState` per-instance mutex | Reduces contention: parallel instances lock independently; no global RwLock bottleneck |
| Executing flag + finish_execute re-check | Ensures no dirty signal is lost in the window between task start and output write, without requiring the task to hold a lock across the apply call |
| `runner.rs` owns all coordination logic | Keeps topology, workstate, value_store independently testable with minimal inter-module coupling |

