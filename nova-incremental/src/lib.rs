//! # Incremental Computation Graph
//!
//! This module provides a **lazy, persistent, parallel incremental computation
//! system** for transforming a set of input values into a set of output values
//! through a directed acyclic graph (DAG) of typed transform functions.
//!
//! ## Quick Start
//!
//! ```no_run
//! use std::sync::Arc;
//! use nova_incremental::{IncrementalEngine, value::Value,
//!     transform::{Transform, OneToOneTransform, TransformError},
//!     storage::MemoryStorage};
//! use async_trait::async_trait;
//!
//! struct Double;
//! #[async_trait]
//! impl OneToOneTransform for Double {
//!     async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
//!         let n = *input.downcast::<i32>().unwrap();
//!         Ok(Value::new(n * 2))
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let storage = Arc::new(MemoryStorage::new());
//!     let mut engine = IncrementalEngine::new(storage);
//!     engine.register_transform("double", Transform::OneToOne(Arc::new(Double)));
//!
//!     let input  = engine.add_input(Value::new(21i32));
//!     let output = engine.add_output_node();
//!     engine.connect(&[input], &[output], "double").unwrap();
//!
//!     engine.update().await;
//!     let v = engine.get_value(output).await.unwrap().unwrap();
//!     assert_eq!(v.downcast::<i32>(), Some(&42i32));
//! }
//! ```
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
#[allow(unused_imports)]
pub use engine::{IncrementalEngine, EngineError};
#[allow(unused_imports)]
pub use node_id::NodeId;
#[allow(unused_imports)]
pub use scheduler::UpdateReport;
#[allow(unused_imports)]
pub use storage::MemoryStorage;
#[allow(unused_imports)]
pub use registry::TransformRegistry;
