# Nova Incremental — Architecture & API Design

> **Status**: Design finalised, implementation in progress.  
> **Crate**: `nova-incremental`  
> **Last revised**: 2026-03-03

---

## Table of Contents

1. [Goals & Non-Goals](#1-goals--non-goals)
2. [Core Concepts](#2-core-concepts)
3. [Public API Surface](#3-public-api-surface)
4. [The `Transform` Trait](#4-the-transform-trait)
5. [TransformSchema Builder](#5-transformschema-builder)
6. [TransformContext — Typed I/O](#6-transformcontext--typed-io)
7. [KeyExtractor — Collection Identity](#7-keyextractor--collection-identity)
8. [EngineBuilder — Static Topology](#8-enginebuilder--static-topology)
9. [Engine — Runtime Operations](#9-engine--runtime-operations)
10. [Storage Trait & Checkpoint System](#10-storage-trait--checkpoint-system)
11. [Incremental Execution Model](#11-incremental-execution-model)
12. [Collection Edge Mechanics](#12-collection-edge-mechanics)
13. [Collection→Collection Wiring (Template Expansion)](#13-collectioncollection-wiring-template-expansion)
14. [Persistence Model](#14-persistence-model)
15. [Nova-Analyze Usage Pattern](#15-nova-analyze-usage-pattern)
16. [Design Rationale & Rejected Alternatives](#16-design-rationale--rejected-alternatives)
17. [Internal Module Map](#17-internal-module-map)

---

## 1. Goals & Non-Goals

### Goals

- **Correctness**: only re-evaluate transforms whose inputs have actually changed (content-hash early exit).
- **Incrementality**: fine-grained per-element tracking inside collection edges so a one-file change doesn't re-process every file.
- **Persistence**: committed engine state survives process restarts; the next run reuses prior computed values where inputs are unchanged.
- **Type safety**: no `Any`-casting, no string type keys, no untyped `Value` objects visible outside the crate.
- **Clean API**: external crates import ~14 public items; all internals are `pub(crate)`.
- **Static topology**: graph structure is declared once via `EngineBuilder`, validated, then sealed. Runtime changes to topology are not supported; only input values change at runtime.

### Non-Goals

- Dynamic graph topology mutation after build (solved architecturally: variable file sets become `Collection` input values, not additional nodes).
- General-purpose dataflow framework (this is optimised for the Nova compiler pipeline).
- Distributed execution.

---

## 2. Core Concepts

### Bipartite Graph

The computation graph is **bipartite**: it alternates between two kinds of entities.

```
[IoNode: Input]  ──edge──►  [TransformNode]  ──edge──►  [IoNode: Output]
                                   ▲
                             [IoNode: Input]
```

- **IoNode** (Input or Output): holds a single value (or a collection of values on a collection-typed edge). Users read/write IoNodes.
- **TransformNode**: owns a `dyn Transform` instance. Reads from upstream IoNodes or TransformNodes; writes to downstream IoNodes or TransformNodes.
- **ValueEdge**: the edge between any two nodes carries the current value payload (a `SingleEdge` or `CollectionEdge`).

Values live **on edges**, not on nodes. A TransformNode is "dirty" when any of its input edges have a value that hasn't been consumed by this transform yet.

### Node Identity

Every node has a `Uuid`-based stable identity. Users supply `Uuid` values (generated via `Uuid::new_v5` for stability across sessions) when declaring nodes in `EngineBuilder`. The engine stores topology keyed by these UUIDs so that persisted values can be correctly associated with nodes on the next startup.

The `Uuid` type is the only identity type in the public API. No `NodeId` newtype is exposed.

### Static Topology

The graph topology (nodes + wires) is **declared once** via `EngineBuilder` and becomes immutable after `build()`. This enables:

- Full validation before any execution.
- Simpler internal representation (no concurrent structural mutations).
- Warm starts: the topology is always re-declared by the builder; only the **cached values** need to be restored from storage.

Variable-size inputs (e.g., a changing set of source files) are modelled as a **single input node** holding a `ProjectDescriptor` (or similar aggregate), which a transform expands into a `Collection`. No new nodes are added when files change.

---

## 3. Public API Surface

The `nova-incremental` crate exports exactly the following items:

```rust
// Engine lifecycle
pub use engine::Engine;
pub use engine::EngineError;
pub use builder::EngineBuilder;

// Update result
pub use runner::UpdateReport;

// Storage backend
pub use storage::Storage;
pub use storage::StorageError;
pub use storage::MemoryStorage;

// Transform authoring
pub use transform::Transform;        // trait to implement
pub use transform::TransformContext; // injected per apply() invocation
pub use transform::TransformError;   // error type returned from apply()
pub use transform::TransformSchema;  // opaque builder, returned by Transform::schema()
pub use transform::IncrementalValue; // auto-impl marker trait for value types
pub use transform::KeyExtractor;     // trait for deriving stable collection element keys
pub use transform::CollectionInput;  // value returned by ctx.input_collection()
pub use transform::CollectionChange; // incremental diff inside CollectionInput

// Stable node identity (re-exported for caller convenience)
pub use uuid::Uuid;
```

**Nothing else is public.** All internal modules (`graph`, `collection`, `value`, `slot`,
`scheduler`, `loader`, `registry`, `cycle`, `node_id`) are `pub(crate)`.

---

## 4. The `Transform` Trait

```rust
/// Implement this trait to define a computation step in the incremental graph.
///
/// ## Schema Declaration
///
/// `schema()` is a static (non-instance, `where Self: Sized`) function called
/// once at registration time. It declares the typed slot layout of this transform.
/// The engine uses it to:
/// - Auto-register all input/output value types (no separate `register_value_type` needed).
/// - Validate wiring at `EngineBuilder::build()` time.
/// - Set up the scheduler's slot assembly logic.
///
/// ## Instance Ownership
///
/// The transform instance is owned by the engine (moved into `EngineBuilder::register`).
/// For stateless transforms use a unit struct. For stateful transforms (e.g., those
/// holding a file-system handle), inject dependencies via the constructor.
///
/// ## Invocation
///
/// For `Single`-typed slots the transform is invoked once per update wave.
/// For `Collection`-typed input slots receiving elements from a `Collection` output,
/// the transform is invoked **once per dirty collection element** (see §12).
#[async_trait]
pub trait Transform: Send + Sync + 'static {
    /// Declare the slot layout of this transform.
    ///
    /// Must not depend on instance state. Called as `MyTransform::schema()`.
    fn schema() -> TransformSchema where Self: Sized;

    /// Execute the transform for one invocation.
    ///
    /// Read inputs via `ctx.input(slot)` / `ctx.input_collection(slot)`.
    /// Write outputs via `ctx.output(slot, value)` / `ctx.output_collection(slot, items)`.
    /// Use `?` to propagate `TransformError`.
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError>;
}
```

### Example: stateless 1-in 1-out

```rust
struct LexTransform;

#[async_trait]
impl Transform for LexTransform {
    fn schema() -> TransformSchema {
        TransformSchema::new()
            .input::<FileContent>()
            .output::<LexOutput>()
    }

    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let content = ctx.input::<FileContent>(0)?;
        let lex = crate::lexical::transform(content.as_str()?)
            .map_err(|e| TransformError::new(format!("lex({}): {}", content.path, e.message)))?;
        ctx.output(0, LexOutput::new(content.path.clone(), lex))
    }
}
```

### Example: stateful transform (injected dependency)

```rust
struct LoadTransform(Arc<dyn FileAccess>);

#[async_trait]
impl Transform for LoadTransform {
    fn schema() -> TransformSchema {
        TransformSchema::new()
            .input::<FileStat>()
            .output::<FileContent>()
    }

    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let stat = ctx.input::<FileStat>(0)?;
        let bytes = self.0.read(&stat.path).await
            .map_err(|e| TransformError::new(e.message))?;
        ctx.output(0, FileContent::new(stat.path.clone(), bytes))
    }
}
```

---

## 5. TransformSchema Builder

`TransformSchema` is a **public but opaque** type. Its fields are private. It is constructed
only via the builder methods shown below, ensuring internal consistency.

```rust
pub struct TransformSchema { /* opaque */ }

impl TransformSchema {
    /// Start building a new schema with no slots.
    pub fn new() -> Self;

    /// Append a single-value input slot of type `T`.
    /// Slots are zero-indexed in the order they are declared.
    pub fn input<T: IncrementalValue>(self) -> Self;

    /// Append a gathered collection input slot of type `T`, identified by key extractor `K`.
    pub fn input_collection<T: IncrementalValue, K: KeyExtractor<T>>(self) -> Self;

    /// Append a single-value output slot of type `T`.
    pub fn output<T: IncrementalValue>(self) -> Self;

    /// Append a spread collection output slot of type `T`, identified by key extractor `K`.
    pub fn output_collection<T: IncrementalValue, K: KeyExtractor<T>>(self) -> Self;
}
```

The `KeyExtractor` type parameter on `input_collection` / `output_collection` binds the key
derivation strategy at compile time. The engine stores the extracted `fn extract_key` function
pointer internally — no string sorter key, no runtime registry lookup.

### `IncrementalValue` marker trait

```rust
/// Marker trait automatically implemented for all types that can flow through
/// the incremental graph.
///
/// No manual implementation needed — any type satisfying the bounds is automatically
/// an `IncrementalValue`.
pub trait IncrementalValue:
    Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static {}

impl<T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static>
    IncrementalValue for T {}
```

---

## 6. TransformContext — Typed I/O

`TransformContext` is constructed by the engine per transform invocation. It holds the
assembled input values and collects output values. Callers never construct it.

```rust
pub struct TransformContext { /* opaque */ }

impl TransformContext {
    /// Read the single value on input slot `slot` as `T`.
    ///
    /// `slot` is the zero-based index matching the order of `.input::<T>()` calls in the schema.
    ///
    /// # Errors
    /// Returns `TransformError` if `slot` is out of range or the stored type does not match `T`.
    /// Use `?` to propagate.
    pub fn input<T: IncrementalValue>(&self, slot: usize) -> Result<&T, TransformError>;

    /// Read the gathered collection on input slot `slot`.
    ///
    /// Returns a `CollectionInput<T>` containing all current elements (in key order)
    /// plus an incremental diff describing what changed since the last evaluation.
    ///
    /// # Errors
    /// Returns `TransformError` if `slot` is out of range, is not a collection slot, or type mismatch.
    pub fn input_collection<T: IncrementalValue>(&self, slot: usize)
        -> Result<CollectionInput<'_, T>, TransformError>;

    /// Write a single value to output slot `slot`.
    ///
    /// `slot` is the zero-based index matching the order of `.output::<T>()` calls in the schema.
    ///
    /// # Errors
    /// Returns `TransformError` if `slot` is out of range or type mismatch.
    pub fn output<T: IncrementalValue>(&mut self, slot: usize, value: T)
        -> Result<(), TransformError>;

    /// Write a collection of values to output slot `slot`.
    ///
    /// The engine applies the `KeyExtractor` declared for this slot to each item to derive
    /// stable element keys automatically. The caller does not manage keys directly.
    ///
    /// # Errors
    /// Returns `TransformError` if `slot` is out of range, is not a collection slot, or type mismatch.
    pub fn output_collection<T: IncrementalValue>(&mut self, slot: usize, items: Vec<T>)
        -> Result<(), TransformError>;
}
```

### Collection types

```rust
/// A gathered collection input: all current elements plus an incremental diff.
pub struct CollectionInput<'a, T> {
    /// All current elements in stable key order (ascending by extracted key).
    pub elements: &'a [T],
    /// What changed since the last time this transform was evaluated.
    pub diff: &'a CollectionChange<T>,
}

/// Incremental diff for a collection slot.
pub struct CollectionChange<T> {
    /// Elements newly added to the collection since the last evaluation.
    pub added: Vec<T>,
    /// Elements that were present before and whose values changed: `(old, new)`.
    pub changed: Vec<(T, T)>,
    /// Stable keys of elements that were removed.
    pub removed: Vec<u64>,
}
```

---

## 7. KeyExtractor — Collection Identity

Every collection slot has an associated `KeyExtractor` that derives a stable `u64` key from
an element. The engine uses this key to:

- Track which elements are new, changed, or removed across evaluations.
- Provide stable deterministic ordering (elements are iterated in ascending key order).
- Associate persisted element data with the correct element on warm start.

```rust
/// Derive a stable, unique `u64` key from a collection element.
///
/// ## Requirements
///
/// - **Stable**: given the same logical element, always returns the same key.
/// - **Unique within a collection**: no two distinct elements in the same collection
///   should share a key (collisions cause incorrect diff behaviour).
/// - **Pure**: must not have side effects.
///
/// ## Implementation note
///
/// Zero-sized types (unit structs) are idiomatic since no state is needed.
pub trait KeyExtractor<T>: Send + Sync + 'static {
    fn extract_key(item: &T) -> u64;
}
```

### Example

```rust
/// Extract key for FileStat by hashing the path.
struct FileStatByPath;

impl KeyExtractor<FileStat> for FileStatByPath {
    fn extract_key(s: &FileStat) -> u64 {
        use std::hash::{Hash, Hasher, DefaultHasher};
        let mut h = DefaultHasher::new();
        s.path.hash(&mut h);
        h.finish()
    }
}
```

---

## 8. EngineBuilder — Static Topology

`EngineBuilder` declares the complete graph topology and registers all transforms. Calling
`build()` validates the topology and returns a sealed `Engine`. After `build()`, no structural
changes to the graph are possible.

```rust
pub struct EngineBuilder { /* opaque */ }

impl EngineBuilder {
    /// Create an empty builder.
    pub fn new() -> Self;

    /// Register a transform type under `key`.
    ///
    /// `key` is a stable string identifier used in persisted topology records.
    /// It must be unique within this builder.
    ///
    /// All value types referenced in `T::schema()` are automatically registered.
    /// No separate value type registration calls are needed.
    ///
    /// `instance` is the transform instance (owned). For stateless transforms
    /// use a unit struct literal. For stateful transforms inject dependencies
    /// via the struct constructor.
    pub fn register<T: Transform>(self, key: &str, instance: T) -> Self;

    /// Declare an input node (user-writable) with stable identity `id`.
    ///
    /// `T` is the value type that will be written via `engine.set_input::<T>(id, value)`.
    pub fn input_node<T: IncrementalValue>(self, id: Uuid) -> Self;

    /// Declare an output node (user-readable, computed by an upstream transform).
    pub fn output_node(self, id: Uuid) -> Self;

    /// Declare a transform node with stable identity `id`, using registered transform `key`.
    pub fn transform_node(self, id: Uuid, key: &str) -> Self;

    /// Wire an IoNode directly to another IoNode (passthrough, no transform).
    pub fn wire(self, from: Uuid, to: Uuid) -> Self;

    /// Wire an IoNode to input slot `slot` of a transform node.
    ///
    /// The engine automatically determines the edge type (Single or Collection)
    /// from the transform's schema for that slot.
    pub fn wire_into_slot(self, from: Uuid, to_transform: Uuid, slot: usize) -> Self;

    /// Wire output slot `slot` of a transform node to an IoNode.
    pub fn wire_slot_to(self, from_transform: Uuid, slot: usize, to: Uuid) -> Self;

    /// Wire output slot of one transform directly into input slot of another.
    pub fn wire_slot_to_slot(
        self,
        from_transform: Uuid, out_slot: usize,
        to_transform:   Uuid, in_slot:  usize,
    ) -> Self;

    /// Set the maximum number of iterations for cyclic formations (default: 1000).
    pub fn cycle_limit(self, limit: u32) -> Self;

    /// Validate the declared topology and open storage.
    ///
    /// Validation checks:
    /// - All referenced transform keys exist.
    /// - All referenced node UUIDs are declared.
    /// - Slot indices are in range.
    /// - Type compatibility: the value type on the source matches the target slot's declared type.
    ///
    /// On warm start (storage has a committed state), cached values are loaded lazily.
    /// The topology is always re-declared via the builder; there is no `restore_graph()`.
    ///
    /// # Errors
    /// Returns `EngineError` if validation fails or storage cannot be opened.
    pub async fn build(self, storage: Arc<dyn Storage>) -> Result<Engine, EngineError>;
}
```

### Warm Start Behaviour

On warm start the builder re-declares the exact same topology as before (same UUIDs, same wiring).
The engine checks storage for previously committed values and loads them into the lazy cache.
Transforms with clean (unchanged-input) cached outputs from the prior session are skipped
by the hash early-exit and will not be re-evaluated. No `restore_graph()` call is needed.

---

## 9. Engine — Runtime Operations

`Engine` is the sealed, immutable-topology runtime. After `build()` no structural changes occur.

```rust
pub struct Engine { /* opaque */ }

impl Engine {
    // -----------------------------------------------------------------------
    // Input updates
    // -----------------------------------------------------------------------

    /// Update the value of an input node.
    ///
    /// If the new value has the same content hash as the current value, the
    /// node is NOT marked dirty — no downstream recomputation is triggered.
    pub fn set_input<T: IncrementalValue>(&self, id: Uuid, value: T)
        -> Result<(), EngineError>;

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    /// Run one incremental update cycle.
    ///
    /// Evaluates all dirty transforms in topological wave order (parallel within
    /// each wave). Returns an `UpdateReport` summarising the work done and any errors.
    pub async fn update(&self) -> UpdateReport;

    // -----------------------------------------------------------------------
    // Value access
    // -----------------------------------------------------------------------

    /// Read the current value of an output (or input) node.
    ///
    /// Returns `None` if the node has not yet been computed.
    /// Returns `Err` if the stored type does not match `T`.
    pub async fn get<T: IncrementalValue>(&self, id: Uuid)
        -> Result<Option<T>, EngineError>;

    // -----------------------------------------------------------------------
    // Checkpoint / persistence
    // -----------------------------------------------------------------------

    /// Begin a checkpoint. At most one may be active at a time.
    pub async fn checkpoint(&self) -> Result<(), EngineError>;

    /// Commit all writes since `checkpoint()` to durable storage.
    pub async fn commit(&self) -> Result<(), EngineError>;

    /// Roll back all writes since `checkpoint()`.
    pub async fn discard(&self) -> Result<(), EngineError>;
}
```

### UpdateReport

```rust
pub struct UpdateReport {
    pub transforms_evaluated:        usize,
    pub transforms_changed:          usize,
    pub transforms_skipped:          usize,
    pub transforms_blocked:          usize,
    pub collection_elements_changed: usize,
    pub errors:                      Vec<(Uuid, TransformError)>,
    pub cycle_limit_exceeded:        Vec<Uuid>,
}

impl UpdateReport {
    /// Returns `true` if there were no errors and no cycle limit violations.
    pub fn is_ok(&self) -> bool;
}
```

---

## 10. Storage Trait & Checkpoint System

```rust
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError>;
    async fn set(&self, key: StorageKey, value: StorageValue) -> Result<(), StorageError>;
    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError>;

    async fn checkpoint(&self) -> Result<(), StorageError>;
    async fn commit(&self)     -> Result<(), StorageError>;
    async fn discard(&self)    -> Result<(), StorageError>;
}

pub struct StorageError { pub message: String }

pub struct MemoryStorage; // in-memory, for tests and ephemeral use
impl MemoryStorage { pub fn new() -> Self; }
```

`StorageKey` and `StorageValue` are opaque — callers never construct them. They are used
internally by the engine's loader and persistence code.

### Checkpoint Lifecycle

```
engine.checkpoint().await?;      // begin staging
engine.set_input(id, val)?;      // value change is staged
engine.update().await;           // computed values are staged as they complete
engine.commit().await?;          // all staged writes become durable
// OR:
engine.discard().await?;         // all staged writes are discarded
```

---

## 11. Incremental Execution Model

### Dirty Tracking

Each `TransformNode` has a status: `Clean`, `Dirty`, or `Error`. Initially all nodes are `Dirty`.

A node becomes `Dirty` when any incoming edge has a new value whose content hash differs from
the hash that was current when the transform last ran.

### Wave Execution

On `engine.update()` the scheduler:

1. Collects all `Dirty` transform nodes and sorts them topologically.
2. Groups them into **waves**: a wave contains all nodes whose upstream transforms are in earlier waves. Nodes within a wave run concurrently via `tokio::spawn`.
3. After each wave, output values are flushed to storage in background tasks (joined before the next wave).
4. A node that errors is marked `Error`; immediate downstream nodes are marked `Blocked` (not re-evaluated, not marked `Error`, remain `Dirty` for the next update).

### Hash Early-Exit

After a transform produces its output, the engine hashes the output value. If the hash matches
the previously stored hash, downstream nodes are **not** dirtied — even though the transform
re-ran. This prevents cascading recomputation when an upstream change doesn't actually affect
the output.

### Cycle Detection

Legal cycles (gather transforms with a collection back-edge) are handled by Tarjan SCC detection.
SCC groups are iterated with a fixed-point loop bounded by the cycle limit.
Convergence: no collection edge in the SCC changes hash between iterations.

---

## 12. Collection Edge Mechanics

A `Collection`-typed edge carries a `CollectionEdge`: a list of `CollectionElement` records, each with:
- A stable `u64` key (derived by `KeyExtractor`).
- The current value payload.
- A content hash.
- A `dirty` flag (set when this element is new or its hash changed).

### Single → Collection (Gather / Fan-In)

When an IoNode connected to a `Collection`-typed input slot pushes a new `Single` value,
the scheduler upserts it into the collection using the element's key:
- New key → `added`. Changed hash → `changed`. Old key no longer fed → `removed`.

Used when many upstream sources funnel into a single gathering transform.

### Collection → Single (Per-Element Expansion / Fan-Out)

When a `Collection`-typed output slot is connected to a `Single`-typed input slot,
the scheduler invokes the transform **once per dirty element**, passing that element's value
as a `Single` input. The outputs are collected back into a collection on the downstream edge.

This is the **template expansion** pattern — one transform definition handles N elements
without requiring N TransformNodes.

---

## 13. Collection→Collection Wiring (Template Expansion)

This is the primary pattern for the Nova compiler pipeline:

```
[project_input: ProjectDescriptor]
    └──[expand]──► Collection<FileStat>
                       └──[load]──► Collection<FileContent>   (per FileStat)
                                        └──[lex]──► Collection<LexOutput>
                                                        └──[parse]──► Collection<ParseOutput>
                                                                           └──[collect]──► String
```

Each arrow is a **single graph edge**. The scheduler handles per-element invocations transparently.

### How it works (expand→load example)

```
builder.wire_slot_to_slot(EXPAND_T, 0, LOAD_T, 0)
```

- `expand` output slot 0 is declared `output_collection::<FileStat, FileStatByPath>()`.
- `load` input slot 0 is declared `input::<FileStat>()` (Single).
- Engine detects: `Collection → Single` crossing.
- Per dirty `FileStat` element, scheduler invokes `LoadTransform::apply()` with that `FileStat` as `ctx.input::<FileStat>(0)`.
- `LoadTransform` writes `ctx.output(0, FileContent {...})`.
- Engine keys the `FileContent` by the same key as the input `FileStat` (from `FileStatByPath::extract_key`) and upserts it into the output collection on the edge toward `lex`.

When the `Collection<FileStat>` changes (file added/changed/removed):
- Only the dirty elements trigger a re-invocation of `load`. Unchanged elements are skipped.
- Changes propagate incrementally through `lex` → `parse` the same way.
- `collect` receives the full `Collection<ParseOutput>` plus a diff of what changed.

---

## 14. Persistence Model

### What is persisted

- **Node values**: every IoNode value is persisted after each transform evaluation within an active checkpoint, keyed by node UUID.
- **Collection element values**: each element of a `CollectionEdge` is persisted individually under `(node_uuid, element_key)` storage keys.
- **Graph topology** (optional, for diagnostics): node/edge structure as UUIDs. Since topology is always re-declared via the builder, this is not needed for correctness but aids warm-start value loading.

### What is NOT persisted

- Transform instances (they are code, reconstructed from the builder every startup).
- Dirty flags (all nodes start dirty; hash early-exit prevents redundant recomputation).
- In-flight `prev_output` hints for cyclic transforms (memory-only, not durable).

### Warm Start Flow

1. `EngineBuilder::build(storage)` reconstructs topology from the builder declarations (always code-driven).
2. Engine pre-populates the lazy loader cache with committed node values from storage.
3. All transforms are initially `Dirty`.
4. On first `engine.update()`:
   - For each dirty transform, the scheduler reads inputs (from cache if available).
   - Computes output, hashes it, compares to stored hash.
   - If hashes match → downstream not dirtied (hash early-exit).
5. Net effect: if no inputs changed, the entire pipeline is traversed in O(n) but produces zero recomputation.

---

## 15. Nova-Analyze Usage Pattern

### Pipeline Topology

```
PROJECT_INPUT_ID: ProjectDescriptor
    └──[EXPAND_T]──► Collection<FileStat>
                         └──[LOAD_T]──► Collection<FileContent>
                                            └──[LEX_T]──► Collection<LexOutput>
                                                               └──[PARSE_T]──► Collection<ParseOutput>
                                                                                    └──[COLLECT_T]──► BUNDLE_OUTPUT_ID: String
```

All UUIDs are stable (derived from a fixed namespace UUID via `Uuid::new_v5`).

### Builder Setup

```rust
const NS: Uuid = Uuid::from_bytes([/* fixed 16 bytes */]);
fn node(name: &str) -> Uuid { Uuid::new_v5(&NS, name.as_bytes()) }

async fn build_engine(fs: Arc<dyn FileAccess>, storage: Arc<dyn Storage>)
    -> Result<Engine, EngineError>
{
    EngineBuilder::new()
        .register("expand",  ExpandTransform)
        .register("load",    LoadTransform(Arc::clone(&fs)))
        .register("lex",     LexTransform)
        .register("parse",   ParseTransform)
        .register("collect", CollectTransform)
        .input_node::<ProjectDescriptor>(node("project_input"))
        .output_node(node("bundle_output"))
        .transform_node(node("expand_t"),  "expand")
        .transform_node(node("load_t"),    "load")
        .transform_node(node("lex_t"),     "lex")
        .transform_node(node("parse_t"),   "parse")
        .transform_node(node("collect_t"), "collect")
        .wire_into_slot(node("project_input"), node("expand_t"),  0)
        .wire_slot_to_slot(node("expand_t"),  0, node("load_t"),    0)
        .wire_slot_to_slot(node("load_t"),    0, node("lex_t"),     0)
        .wire_slot_to_slot(node("lex_t"),     0, node("parse_t"),   0)
        .wire_slot_to_slot(node("parse_t"),   0, node("collect_t"), 0)
        .wire_slot_to(node("collect_t"), 0, node("bundle_output"))
        .build(storage)
        .await
}
```

### SemanticSession

```rust
pub struct SemanticSession {
    engine: Engine,
}

impl SemanticSession {
    pub async fn open(storage: Arc<dyn Storage>, fs: Arc<dyn FileAccess>)
        -> Result<Self, EngineError>
    {
        Ok(Self { engine: build_engine(fs, storage).await? })
    }

    pub fn set_files(&self, files: Vec<FileStat>) -> Result<(), EngineError> {
        self.engine.set_input(node("project_input"), ProjectDescriptor { files })
    }

    pub async fn run(&self) -> UpdateReport { self.engine.update().await }

    pub async fn get_bundle(&self) -> Result<Option<String>, EngineError> {
        self.engine.get::<String>(node("bundle_output")).await
    }

    pub async fn checkpoint(&self) -> Result<(), EngineError> { self.engine.checkpoint().await }
    pub async fn commit(&self)     -> Result<(), EngineError> { self.engine.commit().await }
    pub async fn discard(&self)    -> Result<(), EngineError> { self.engine.discard().await }
}
```

### Transform Implementations

```rust
// ---- expand: ProjectDescriptor → Collection<FileStat> ----
struct FileStatByPath;
impl KeyExtractor<FileStat> for FileStatByPath {
    fn extract_key(s: &FileStat) -> u64 { /* hash s.path */ }
}

struct ExpandTransform;
#[async_trait] impl Transform for ExpandTransform {
    fn schema() -> TransformSchema {
        TransformSchema::new()
            .input::<ProjectDescriptor>()
            .output_collection::<FileStat, FileStatByPath>()
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let desc = ctx.input::<ProjectDescriptor>(0)?;
        ctx.output_collection(0, desc.files.clone())
    }
}

// ---- load: FileStat → FileContent (invoked per element) ----
struct LoadTransform(Arc<dyn FileAccess>);
#[async_trait] impl Transform for LoadTransform {
    fn schema() -> TransformSchema {
        TransformSchema::new().input::<FileStat>().output::<FileContent>()
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let stat = ctx.input::<FileStat>(0)?;
        let bytes = self.0.read(&stat.path).await.map_err(|e| TransformError::new(e.message))?;
        ctx.output(0, FileContent::new(stat.path.clone(), bytes))
    }
}

// ---- collect: Collection<ParseOutput> → String ----
struct ParseOutputByPath;
impl KeyExtractor<ParseOutput> for ParseOutputByPath {
    fn extract_key(p: &ParseOutput) -> u64 { /* hash p.path */ }
}

struct CollectTransform;
#[async_trait] impl Transform for CollectTransform {
    fn schema() -> TransformSchema {
        TransformSchema::new()
            .input_collection::<ParseOutput, ParseOutputByPath>()
            .output::<String>()
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let col = ctx.input_collection::<ParseOutput>(0)?;
        let parts = col.elements.iter()
            .map(|po| Ok(format!("## {}\n{}", po.path,
                encode(&po.result).map_err(|e| TransformError::new(e.to_string()))?)))
            .collect::<Result<Vec<_>, TransformError>>()?;
        ctx.output(0, parts.join("\n\n"))
    }
}
```

---

## 16. Design Rationale & Rejected Alternatives

### Static topology instead of dynamic node add/remove

**Rejected**: dynamic `engine.add_node()` / `engine.remove_node()` (original design).  
**Reason**: dynamic topology mutations during concurrent execution require heavy synchronisation.
Static topology simplifies the scheduler, enables build-time validation, and makes warm-start
trivial (topology is code; only values are persisted).  
**Solution**: variable file sets become a `Collection` input. An `"expand"` transform unpacks
a `ProjectDescriptor` into `Collection<FileStat>`. Per-element fan-out handles N files with
a fixed graph.

### `KeyExtractor` instead of a sorter function

**Rejected**: `Sorter<T>: Fn(&T, &T) -> Ordering` registered by string key.  
**Reason**: order doesn't matter for correctness — only stable element identity matters.
The sorter model implied order-is-semantic. `KeyExtractor` is simpler (just a hash), more
honest about the actual invariant, and eliminates the sorter string-key registry entirely.

### `Transform` trait instead of typed closure adapter

**Rejected**: `TypedTransform<In, Out, F, Fut>` + `register_transform(key, Arc<dyn TransformFn>)`.  
**Reason**: the closure adapter required exposing `SlotInput`, `SlotOutput`, `Value`,
`ValueTypeRegistry` to construct. The trait approach keeps all type machinery internal; callers
only see `TransformContext` with typed accessor methods.

### Automatic value type registration

**Rejected**: separate `engine.register_value_type::<T>("key")` calls.  
**Reason**: error-prone and redundant. The type information is already in `TransformSchema`
(e.g., `.input::<FileStat>()` already knows `T = FileStat`). Auto-registration at
`builder.register::<T>()` time is safer and removes boilerplate.

### `Uuid` instead of `NodeId` newtype in public API

**Rejected**: `pub struct NodeId(Uuid)` as the public identity type.  
**Reason**: since edge IDs are not in the public API, there is only one kind of identifier in
the public surface. A newtype adds ceremony without preventing misuse. Raw `Uuid` from the
well-known `uuid` crate is more ergonomic; `NodeId` remains a `pub(crate)` internal alias.

### `EngineBuilder` instead of building on a live `Engine`

**Rejected**: `engine.add_transform_node_with_id(id, key)` + `engine.graph().add_single_edge(...)`.  
**Reason**: exposes `Graph`, `Endpoint`, `EdgeId` and other internal types. The builder approach
hides all graph internals, validates the entire topology before execution, and catches wiring
errors at build time.

---

## 17. Internal Module Map

All modules are `pub(crate)` (declared as `mod`, not `pub mod`, in `lib.rs`).

| Module | Responsibility |
|--------|---------------|
| `engine.rs` | `Engine` implementation; delegates to graph/scheduler/loader |
| `builder.rs` | `EngineBuilder`; validates declared topology, constructs `Engine` |
| `graph.rs` | Bipartite DAG: `IoNode`, `TransformNode`, `ValueEdge`, dirty tracking |
| `scheduler.rs` | Wave-parallel + SCC fixed-point update loop |
| `loader.rs` | Two-level value cache (in-memory + `Storage` backend) |
| `transform_internal.rs` | Internal transform wrapper, `SlotInput`, `SlotOutput` |
| `value.rs` | Type-erased `Value`, `ValueTypeRegistry`, serde vtable |
| `slot.rs` | `InternalSlotDescriptor`, `InternalTransformSchema` |
| `collection.rs` | `CollectionEdge`, `CollectionElement`, internal diff types |
| `registry.rs` | Transform registry (key → internal transform wrapper) |
| `cycle.rs` | Tarjan SCC detection for legal cyclic formations |
| `node_id.rs` | `NodeId` newtype (internal alias for `Uuid`) |
| `storage.rs` | `Storage` trait + `MemoryStorage` + serialisation helpers |
| `tests.rs` | Integration tests |

---

*End of document.*
