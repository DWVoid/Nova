//! # Incremental Computation Graph
//!
//! This module provides a **lazy, persistent, parallel incremental computation
//! system** for transforming a set of input values into a set of output values
//! through a directed acyclic graph (DAG) of typed transform functions.
//!
//! ## Architecture Overview
//!
//! ```text
//! User code
//!    │
//!    ▼
//! IncrementalEngine          (engine.rs)   – public façade
//!    ├── Graph               (graph.rs)    – DAG topology + dirty flags
//!    ├── TransformRegistry   (registry.rs) – name → Transform dispatch
//!    ├── LazyLoader          (loader.rs)   – in-memory cache + Storage I/O
//!    │       └── Arc<dyn Storage>          – user-supplied backend
//!    └── Scheduler           (scheduler.rs)– parallel wave execution
//! ```
//!
//! ### Key Concepts
//!
//! | Concept | File | Summary |
//! |---------|------|---------|
//! | `NodeId` | `node_id.rs` | UUID-based stable node identity |
//! | `Value` | `value.rs` | Type-erased `Arc<dyn Any>` node value |
//! | `Transform` | `transform.rs` | One of four arity variants (1→1, 1→N, N→1, N→M) |
//! | `Graph` | `graph.rs` | Lock-free DAG with eager dirty propagation |
//! | `Storage` | `storage.rs` | Async key-value trait (user-supplied) |
//! | `LazyLoader` | `loader.rs` | Two-level value cache (memory + storage) |
//! | `TransformRegistry` | `registry.rs` | Maps stable string keys to live transforms |
//! | `Scheduler` | `scheduler.rs` | Wave-parallel recomputation engine |
//! | `IncrementalEngine` | `engine.rs` | Single user-facing entry point |
//!
//! ### Incremental Update Lifecycle
//!
//! 1. **Input change**: `engine.set_input(id, new_value)` stores the value,
//!    marks the node dirty, and eagerly BFS-propagates the dirty flag to all
//!    transitive dependents.
//! 2. **Schedule**: `engine.update()` calls `Graph::dirty_nodes_topo()` which
//!    returns all dirty nodes in topological order using Kahn's algorithm.
//! 3. **Wave decomposition**: nodes are grouped into parallel waves; all nodes
//!    in a wave have no unresolved dirty predecessors in earlier waves.
//! 4. **Parallel execution**: each wave's nodes are spawned as independent
//!    `tokio::spawn` tasks.  Input values are loaded from `LazyLoader`, the
//!    transform is invoked, and outputs are hashed.
//! 5. **Hash-based early exit**: if the new output hash matches the stored
//!    hash from the last run, the output node is marked clean *without*
//!    propagating dirty to its successors, short-circuiting the rest of the
//!    graph.
//! 6. **Persistence**: changed outputs are written to `Storage` via
//!    `LazyLoader::persist`.  Call `engine.save()` to also persist the graph
//!    topology (required for `IncrementalEngine::load`).

pub mod engine;
pub mod graph;
pub mod loader;
pub mod node_id;
pub mod registry;
pub mod scheduler;
pub mod storage;
pub mod transform;
pub mod value;
mod tests;

// Top-level re-exports for convenience.
pub use engine::{IncrementalEngine, EngineError};
pub use node_id::NodeId;
pub use scheduler::UpdateReport;
pub use storage::MemoryStorage;
// Value, Transform, and TransformRegistry are pub(crate) implementation details.
// TransformError remains public so users can construct/inspect it in transform closures.
pub use transform::TransformError;