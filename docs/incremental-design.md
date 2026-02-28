# Incremental Computation Graph – Design Document
> **Module path**: `src/incremental/`
This document explains the design rationale for every component of the
incremental computation system in `Nova`.  Read it alongside the inline
`//!` module documentation in each source file.
---
## 1. Problem Statement
The Nova compiler needs to re-process source files incrementally: when the
user edits a single file, only the passes that are actually affected by that
change should re-run.  Blindly re-running the entire pipeline on every
keystroke is too slow for interactive use.
This module provides a general-purpose **incremental computation graph**:
a directed acyclic graph (DAG) where nodes hold typed values and edges
represent transform functions.  When an input node changes, only the
transitive dependents are recomputed.
---
## 2. Architecture
```
User code
   │
   ▼
IncrementalEngine          (engine.rs)   – public façade
   ├── Graph               (graph.rs)    – DAG topology + dirty flags
   ├── TransformRegistry   (registry.rs) – name → Transform dispatch
   ├── LazyLoader          (loader.rs)   – in-memory cache + Storage I/O
   │       └── Arc<dyn Storage>          – user-supplied backend
   └── Scheduler           (scheduler.rs)– parallel wave execution
```
### File Summary
| File | Lines | Responsibility |
|------|-------|----------------|
| `mod.rs` | ~120 | Public façade, re-exports, module-level docs |
| `node_id.rs` | ~130 | `NodeId` (UUID newtype) |
| `value.rs` | ~150 | `Value` (type-erased `Arc<dyn Any>`), `ValueHash` |
| `transform.rs` | ~190 | Four arity traits + `Transform` enum |
| `graph.rs` | ~400 | DAG topology, dirty propagation, topological sort |
| `storage.rs` | ~230 | `Storage` trait, `MemoryStorage`, serde helpers |
| `loader.rs` | ~180 | Two-level value cache |
| `registry.rs` | ~80 | `String → Transform` registry |
| `scheduler.rs` | ~350 | Wave-parallel recomputation engine |
| `engine.rs` | ~390 | `IncrementalEngine` façade, save/load |
---
## 3. Key Design Decisions
### 3.1 Type-Erased Values (`value.rs`)
Node values are stored as `Arc<dyn Any + Send + Sync>`.
**Why `Arc`?**  Cheap cloning (pointer copy) lets the scheduler share input
values across multiple Tokio tasks without deep copies.
**Why `dyn Any`?**  The graph must store heterogeneous value types (token
streams, ASTs, diagnostics) without making `Graph` itself generic.  A
monomorphic generic `Graph<T>` would explode into separate graph types for
every value type.
**Why not `Box<dyn Any>`?**  `Box` does not support cheap cloning.
### 3.2 Four Transform Arities (`transform.rs`)
Real compiler pipelines need all four shapes:
| Arity | Sources → Targets | Example |
|-------|-------------------|---------|
| `OneToOne` | 1 → 1 | Parse a single source file |
| `OneToMany` | 1 → N | Split a file into top-level items |
| `ManyToOne` | N → 1 | Link N modules into one IR |
| `ManyToMany` | N → M | Desugar pass (N inputs → M outputs) |
All four traits are `async` via `async-trait` so transforms can perform I/O
(e.g. reading imports through a VFS) without blocking the Tokio runtime.
The `Transform` enum wraps an `Arc<dyn Trait>` for each arity, enabling:
- Uniform storage in edge entries (no per-edge type parameter).
- Static arity dispatch at scheduling time.
- Cheap cloning.
### 3.3 UUID Node IDs (`node_id.rs`)
`NodeId` is a `uuid::Uuid` newtype.
**Why UUID over sequential integers?**
- Stable across process restarts (no shared counter needed).
- Safe to generate in parallel without coordination.
- Globally unique across independently-constructed sub-graphs.
- UUID v5 (name-based) gives reproducible IDs for well-known nodes such as
  input nodes keyed by file path.
### 3.4 Lock-Free Graph (`graph.rs`, `DashMap`)
`Graph` uses `dashmap::DashMap<NodeId, NodeEntry>` rather than a global
`RwLock<HashMap>`.  DashMap shards its internal hash map into 64 buckets,
each with its own shard lock.  During parallel recomputation (see §3.6),
tasks working on different nodes contend on different shards, dramatically
reducing lock contention.
Writes (marking nodes dirty, storing computed values) still acquire a shard
lock, but they are brief and allocation-free.
### 3.5 Eager Dirty Propagation (`graph.rs`)
When `set_input` or `mark_dirty` is called, the dirty flag is immediately
BFS-propagated forward through all transitive dependents.
**Alternative**: lazy propagation at scheduling time.
**Why eager?**  Eager propagation means the recomputation phase is
read-only with respect to the dirty flags (no topology traversal during
the update).  The scheduler just reads a pre-computed list.  The cost
is O(changed nodes × average fan-out) work at write time, which is
typically small.
**Short-circuit**: BFS terminates as soon as it reaches a node that is
already dirty (it and its subtree were already propagated earlier).
### 3.6 Wave-Based Parallel Execution (`scheduler.rs`)
Dirty nodes are split into **topological waves**:
- **Wave 0**: nodes with no dirty predecessors (typically input nodes).
- **Wave K**: nodes whose dirty predecessors all belong to waves < K.
All nodes within a wave are mutually independent and are spawned as separate
`tokio::task::spawn` tasks.  The scheduler `join_all`s each wave before
proceeding to the next.
**Why Tokio tasks, not Rayon?**  Transforms are `async`; Rayon is designed
for synchronous, CPU-bound work.  Mixing Rayon and Tokio requires
`spawn_blocking`, which adds overhead.  Tokio's multi-threaded work-stealing
scheduler provides similar CPU-level parallelism for compute-bound transforms
while natively supporting async I/O transforms.
### 3.7 Hash-Based Early Exit (`scheduler.rs`)
After a transform runs, the scheduler hashes the output `Value` and compares
it to the stored hash from the previous run.  If they match, the output node
is marked **clean without propagating dirty downstream**.
This means: if a lexer produces the same token stream after a whitespace
change, the parser and all downstream passes are completely skipped.  This
is the "minimal recomputation" property.
**Current hashing strategy**: the erased-pointer address of the `Arc`
allocation is hashed.  This is a conservative approximation: a new
allocation always produces a different hash, even if the value is logically
equal.  Callers that need value-level equality should use
`HashedValue::new(v)` which hashes the concrete type via `std::hash::Hash`.
### 3.8 Storage Trait (`storage.rs`)
The key-value storage backend is exposed as an `async-trait` trait:
```rust
#[async_trait]
pub trait Storage: Send + Sync {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError>;
    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError>;
    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError>;
    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError>;
}
```
The engine accepts an `Arc<dyn Storage>` so users can provide any backend:
file system, SQLite, Redis, cloud object store, etc.  A `MemoryStorage`
implementation is provided for tests and examples.
### 3.9 Lazy Loading (`loader.rs`)
`LazyLoader` maintains a two-level cache:
1. **In-memory `DashMap`** – hot values accessible without any I/O.
2. **Persistent `Storage`** – cold values reloaded on demand.
Node values are only fetched from storage when accessed.  Cold nodes (never
accessed since startup) never consume RAM.  Callers can call `evict` to
release an entry from the in-memory cache (it remains in storage).
### 3.10 Transform Registry (`registry.rs`)
Transform functions are code and cannot be serialised.  The registry maps
user-assigned stable string keys to live `Transform` objects.  When a graph
is persisted, only the key string is stored per edge.  On reload, the user
re-registers all transforms under the same keys, and the loader re-associates
them.
This approach mirrors the **Salsa** incremental compilation framework's
separation of "query definitions" (code) from "query results" (data).
### 3.11 NodeStatus Error State (`graph.rs`)
A failed transform transitions the target node to `NodeStatus::Error` rather
than leaving it `Dirty`.  This prevents the scheduler from retrying the same
deterministically-failing transform on every `update()` call.
The error is automatically cleared (node becomes `Dirty`) when any of its
upstream inputs change, which re-triggers computation.
---
## 4. Data Flow: Incremental Update Cycle
```
set_input(id, new_value)
   │
   ├─ Graph::set_input → stores value in NodeEntry
   └─ Graph::propagate_dirty → BFS marks all transitively dependent nodes Dirty
          │
          ▼
engine.update()
   │
   ├─ Graph::dirty_nodes_topo() → Kahn's algorithm → topologically sorted Vec<NodeId>
   │
   └─ Scheduler::run_update()
          │
          ├─ compute_waves() → split into parallel waves
          │
          └─ for each wave:
                 tokio::spawn per node
                    │
                    ├─ LazyLoader::get(source_ids) → cache hit or storage decode
                    ├─ Transform::apply(inputs) → Vec<Value>
                    ├─ hash_value_erased(output)
                    │
                    ├─ [if hash unchanged] → store_value(clean), skip downstream
                    └─ [if hash changed]  → store_value, cache_value, mark_dirty_downstream
                          │
                          ▼
                    (downstream nodes will be in a later wave)
```
---
## 5. Persistence Lifecycle
### Saving
```
engine.save()
   ├─ Graph::all_edges() → serialize PersistedGraphMeta → Storage::set("__graph_meta__")
   └─ for each node with in-memory value:
          LazyLoader::persist(node_id, value, bytes, hash, is_input)
             └─ Storage::set(node_id.to_storage_key(), PersistedNodeData { ... })
```
### Loading
```
IncrementalEngine::load(storage, registry)
   ├─ Storage::get("__graph_meta__") → deserialize PersistedGraphMeta
   ├─ for each node_id:
   │      LazyLoader::load_node_data(node_id) → PersistedNodeData
   │      Graph::register_node(NodeEntry { last_known_hash: data.value_hash })
   └─ for each edge:
          registry.get(transform_key) → Transform
          Graph::register_edge(EdgeEntry) + update adjacency lists
```
After loading, nodes with a known previous hash will benefit from hash-based
early exit on the first `update()` call (unchanged outputs won't propagate
dirty downstream).
---
## 6. Testing Strategy
Each file has an inline `#[cfg(test)]` module covering:
| File | Tests |
|------|-------|
| `node_id.rs` | Uniqueness, determinism, display, serde round-trip |
| `value.rs` | Downcast, clone sharing, hash determinism |
| `transform.rs` | All four arities, error propagation |
| `graph.rs` | Node/edge creation, adjacency, dirty propagation, topo sort |
| `storage.rs` | MemoryStorage CRUD, encode/decode round-trip |
| `loader.rs` | Cache hits/misses, persist→evict→reload cycle |
| `registry.rs` | Register, get, overwrite, iterate |
| `scheduler.rs` | Wave computation, single-node update, empty graph |
| `engine.rs` | End-to-end pipeline, dirty propagation, save/reload |
---
## 7. Known Limitations and Future Work
| Limitation | Notes |
|-----------|-------|
| Pointer-address hashing | Two transforms that return logically-equal values but as new allocations will unnecessarily propagate dirty. Fix: require `Hash` on value types or use `HashedValue`. |
| No cycle detection | The graph assumes a DAG. Adding a cycle will cause `dirty_nodes_topo` to silently drop cycle members (Kahn's algorithm discards nodes with non-zero in-degree at termination). A cycle-detection pass should be added to `add_transform`. |
| Graph-level transactions | There is no atomic "batch input update + update" primitive. Adding one (e.g. `engine.with_inputs(|b| { b.set(...); b.set(...); })`) would avoid spurious intermediate update cycles. |
| Value serialization | `engine.save()` can only persist values that were previously stored via `persist_serde`. Arbitrary `Value` objects (e.g. i32 set via `add_input`) lose their bytes after restart. Fix: require a user-provided serializer per input node type, or use a serialization registry. |
