# Nova Incremental — Final Redesign Plan (Confirmed)

## Confirmed Design Decisions

| # | Decision |
|---|----------|
| Topology | Bipartite: `InputNode`/`OutputNode`/`TransformNode` are nodes; values live ON edges (`ValueEdge`) |
| Edge connections | `InputNode→Transform`, `Transform→Transform`, `Transform→OutputNode`, `InputNode→OutputNode` all valid |
| No Wire nodes | Intermediate cached values live on edges, not on intermediate nodes |
| Spread | Option C: fixed topology, collection edges track per-element dirty state, transforms receive full collection + `CollectionDiff` |
| Nested collections | Supported: `Single→Collection` and `Collection→Single` transforms are valid; collection elements can themselves be collections |
| Cycle convergence | Re-evaluate gather transform whenever its collection changes; transform receives `prev_output` as hint (in-memory only, not persisted) |
| Gather sorter | `Fn(&Value, &Value) -> Ordering` registered comparator, required per Collection input slot |
| Collection persistence | Per-element, with a separate index record per collection edge |
| Manual wiring | No autowire; users call explicit `add_*_node` + `connect_slot` API |
| Backward compat | `register_one_to_one`/`connect` shims retained |

---

## 1. Graph Topology

```
InputNode ──[ValueEdge]──► TransformNode ──[ValueEdge]──► TransformNode ──[ValueEdge]──► OutputNode
                                │                               ▲
                                └───────────[back-edge, Collection input slot, legal cycle]
```

**Node kinds** (all use `NodeId`):
- `InputNode` — user-supplied value, no incoming edge
- `OutputNode` — user-visible result, no outgoing edge to transforms
- `TransformNode` — holds transform fn + slot schema, has `NodeStatus`

**EdgeEndpoint** on a `ValueEdge`:
- `Endpoint::Node(NodeId)` — for `InputNode` or `OutputNode`
- `Endpoint::TransformInput { transform: NodeId, slot: usize }` — input slot of a TransformNode
- `Endpoint::TransformOutput { transform: NodeId, slot: usize }` — output slot of a TransformNode

---

## 2. Slot System — `slot.rs`

```rust
pub enum SlotKind {
    Single,
    Collection { sorter_key: String },
}

pub struct SlotDescriptor {
    pub kind:     SlotKind,
    pub type_key: String,
}

pub struct TransformSchema {
    pub inputs:  Vec<SlotDescriptor>,
    pub outputs: Vec<SlotDescriptor>,
}
```

**Nested collection note**: A `Collection` slot whose `type_key` refers to a collection-typed value naturally supports nesting. The `CollectionElement.nested` field in `collection.rs` handles the inner layer. Multiple transforms can each contribute `Single→Collection` outputs that gather into one Collection input slot of a downstream transform.

---

## 3. Value Edge — `graph.rs`

```rust
pub struct ValueEdge {
    pub id:      EdgeId,
    pub from:    Endpoint,
    pub to:      Endpoint,
    pub payload: EdgePayload,
}

pub enum EdgePayload {
    Single(SingleEdge),
    Collection(CollectionEdge),  // defined in collection.rs
}

pub struct SingleEdge {
    pub value:      Option<Value>,
    pub value_hash: Option<ValueHash>,
    pub dirty:      bool,
}
```

Dirty propagation is edge-centric: setting an input value marks `SingleEdge.dirty = true`, which marks the downstream TransformNode `Dirty`. After a transform runs and emits output, the scheduler stores the result on the outgoing edge, computes the hash, and marks downstream TransformNodes dirty only if hash changed.

---

## 4. Collection Edge — `collection.rs`

```rust
pub struct CollectionEdge {
    pub elements:  Vec<CollectionElement>,   // sorted by sorter
    pub full_hash: ValueHash,
    pub dirty:     bool,
}

pub struct CollectionElement {
    pub key:    ElementKey,          // sorter(&value) result
    pub value:  Value,
    pub hash:   ValueHash,
    pub dirty:  bool,
    pub nested: Option<Box<CollectionEdge>>,   // for nested collections
}

pub struct CollectionDiff {
    pub added:   Vec<(ElementKey, Value)>,
    pub removed: Vec<ElementKey>,
    pub changed: Vec<(ElementKey, Value, Value)>,  // (key, old, new)
}
```

---

## 5. Transform Function — `transform.rs`

```rust
#[async_trait]
pub(crate) trait TransformFn: Send + Sync {
    fn schema(&self) -> &TransformSchema;
    async fn apply(
        &self,
        inputs:      &[SlotInput],
        prev_output: Option<&[SlotOutput]>,
    ) -> Result<Vec<SlotOutput>, TransformError>;
}

pub(crate) enum SlotInput {
    Single(Value),
    Collection { elements: Vec<Value>, diff: CollectionDiff },
}

pub(crate) enum SlotOutput {
    Single(Value),
    Collection(Vec<(ElementKey, Value)>),  // sorted by sorter
}
```

---

## 6. Cycle Detection — `cycle.rs`

- Tarjan's SCC over `TransformNode` dependency graph (run at topology-change time).
- A cycle is **legal** iff the back-edge targets a `Collection`-typed input slot.
- Legal SCCs stored in `Graph::scc_groups: Vec<SccGroup>`.
- Fixed-point loop in `Scheduler::run_update`:
  ```
  for each dirty SCC group:
    iteration = 0
    loop:
      run wave over dirty TransformNodes in SCC
      if no CollectionEdge in SCC changed → converged, break
      if iteration >= cycle_limit → record error, break
      iteration += 1
  ```
- Default cycle limit: 1000 (configurable via `engine.set_cycle_limit(n)`).

---

## 7. Scheduler — `scheduler.rs`

- `dirty_transforms_topo()`: Kahn's over TransformNodes only.
- `evaluate_transform()`: load SlotInputs from incoming edges → run transform → store SlotOutputs on outgoing edges → hash-compare → mark downstream dirty.
- SCC fixed-point loop runs after the main wave loop.
- `prev_output` cache: `HashMap<NodeId, Vec<SlotOutput>>`, lives for one `update()` call.
- `UpdateReport` extended with `cycles_iterated`, `collection_elements_changed`, `cycle_limit_exceeded`.

---

## 8. Engine API — `engine.rs`

```rust
// Node creation
pub fn add_input_node<T: ...>(&self, value: T) -> Result<NodeId, EngineError>;
pub fn add_output_node(&self) -> NodeId;
pub fn add_transform_node(&self, key: &str) -> Result<NodeId, EngineError>;

// Slot wiring (manual)
pub fn connect_single_input(&self, from: NodeId, to_transform: NodeId, slot: usize) -> Result<EdgeId, EngineError>;
pub fn connect_single_output(&self, from_transform: NodeId, slot: usize, to: NodeId) -> Result<EdgeId, EngineError>;
pub fn connect_collection_input(&self, from: NodeId, to_transform: NodeId, slot: usize) -> Result<EdgeId, EngineError>;
pub fn connect_collection_output(&self, from_transform: NodeId, slot: usize, to: NodeId) -> Result<EdgeId, EngineError>;
// Direct transform-to-transform
pub fn connect_transform_to_transform(&self, from: NodeId, out_slot: usize, to: NodeId, in_slot: usize) -> Result<EdgeId, EngineError>;

// Registration
pub fn register_transform(&mut self, key: &str, schema: TransformSchema, f: impl TransformFn + 'static) -> Result<(), EngineError>;
pub fn register_sorter(&mut self, key: &str, cmp: impl Fn(&Value, &Value) -> Ordering + Send + Sync + 'static);
pub fn set_cycle_limit(&mut self, limit: u32);

// Value access
pub async fn get_value<T>(&self, id: NodeId) -> Result<Option<T>, EngineError>;
pub async fn get_collection<T>(&self, id: NodeId) -> Result<Vec<T>, EngineError>;

// Backward-compat shims (no deprecation yet)
pub fn register_one_to_one<In, Out, F, Fut>(&mut self, key: &str, f: F) -> Result<(), EngineError>;
pub fn register_many_to_one / one_to_many / many_to_many ...;
pub fn connect(&self, sources: &[NodeId], targets: &[NodeId], key: &str) -> Result<(), EngineError>;
```

---

## 9. Persistence — `storage.rs`

```rust
pub struct PersistedGraphMeta {
    pub input_nodes:     Vec<NodeId>,
    pub output_nodes:    Vec<NodeId>,
    pub transform_nodes: Vec<PersistedTransformNode>,
    pub edges:           Vec<PersistedValueEdge>,
    pub scc_groups:      Vec<Vec<NodeId>>,
    pub cycle_limit:     u32,
}

pub struct PersistedTransformNode {
    pub node_id:       NodeId,
    pub transform_key: String,
}

pub struct PersistedValueEdge {
    pub id:       EdgeId,
    pub from:     PersistedEndpoint,
    pub to:       PersistedEndpoint,
    pub kind:     PersistedEdgeKind,   // Single | Collection
}

pub enum PersistedEdgeKind {
    Single { type_key: String, value_hash: ValueHash },
    Collection { element_count: usize, full_hash: ValueHash },
}

// Per-element stored separately under StorageKey::for_collection_element(edge_id, element_key)
pub struct PersistedCollectionElement {
    pub key:             ElementKey,
    pub type_key:        String,
    pub value_bytes:     Vec<u8>,
    pub value_hash:      ValueHash,
    pub has_nested:      bool,
}
```

Single edge values stored under `StorageKey::for_edge(edge_id)`. Collection elements stored under `StorageKey::for_collection_element(edge_id, element_key)`.

---

## 10. File-by-File Summary

| File | Change |
|------|--------|
| `node_id.rs` | Unchanged |
| `graph.rs` | Full rewrite — bipartite; `InputNode`, `OutputNode`, `TransformNode`, `ValueEdge`; new dirty propagation; `dirty_transforms_topo`; SCC cache |
| `transform.rs` | Full rewrite — `TransformFn` trait; `SlotInput`/`SlotOutput`; `TypedTransform`; keep `TransformError` |
| `scheduler.rs` | Major rewrite — `evaluate_transform`; SCC fixed-point; `prev_output` cache; extended `UpdateReport` |
| `engine.rs` | Major rewrite — new node/slot API; `register_transform`/`register_sorter`; backward-compat shims; updated save/load |
| `storage.rs` | Significant — `PersistedValueEdge`; `PersistedTransformNode`; per-element keys; updated `PersistedGraphMeta` |
| `loader.rs` | Moderate — edge-keyed cache; `get_collection`/`persist_collection`; `prev_output` cache |
| `value.rs` | Minor — unchanged core; `SlotInput`/`SlotOutput` may move here |
| `registry.rs` | Moderate — `SorterRegistry`; `TransformSchema` per entry |
| `slot.rs` (new) | `SlotKind`, `SlotDescriptor`, `TransformSchema` |
| `collection.rs` (new) | `CollectionEdge`, `CollectionElement`, `CollectionDiff`, `ElementKey` |
| `cycle.rs` (new) | Tarjan SCC, `CycleValidator`, convergence check |
| `lib.rs` | Export new public types |
| `tests.rs` | Full rewrite + new collection/cycle tests |
