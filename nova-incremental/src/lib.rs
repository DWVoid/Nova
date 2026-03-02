//! # Incremental Computation Graph
//!
//! A lazy, persistent, parallel incremental computation system built on a
//! **bipartite graph** of transform nodes and value edges.
//!
//! ## Architecture
//!
//! ```text
//! IncrementalEngine          (engine.rs)   – public façade
//!    ├── Graph               (graph.rs)    – bipartite DAG + dirty flags
//!    ├── TransformRegistry   (registry.rs) – name → Transform dispatch
//!    ├── SorterRegistry      (registry.rs) – name → comparator
//!    ├── LazyLoader          (loader.rs)   – in-memory cache + Storage I/O
//!    │       └── Arc<dyn Storage>
//!    └── Scheduler           (scheduler.rs)– wave-parallel + SCC fixed-point
//! ```
//!
//! ## Key Concepts
//!
//! | Concept | File | Summary |
//! |---------|------|---------|
//! | `NodeId` | `node_id.rs` | UUID-based stable node identity |
//! | `Value` | `value.rs` | Type-erased `Arc<dyn Any>` value |
//! | `TransformFn` | `transform.rs` | Arbitrary-slot async transform trait |
//! | `SlotInput/Output` | `transform.rs` | Single or Collection slot values |
//! | `TransformSchema` | `slot.rs` | Slot type/kind declarations |
//! | `CollectionEdge` | `collection.rs` | Variable-length edge with per-element diff |
//! | `Graph` | `graph.rs` | Bipartite InputNode/OutputNode/TransformNode graph |
//! | `Storage` | `storage.rs` | Async key-value trait (user-supplied) |
//! | `LazyLoader` | `loader.rs` | Two-level value cache (memory + storage) |
//! | `Scheduler` | `scheduler.rs` | Wave-parallel + SCC fixed-point engine |
//! | `IncrementalEngine` | `engine.rs` | Single user-facing entry point |

pub mod collection;
pub mod cycle;
pub mod engine;
pub mod graph;
pub mod loader;
pub mod node_id;
pub mod registry;
pub mod scheduler;
pub mod slot;
pub mod storage;
pub mod transform;
pub mod value;
mod tests;

pub use engine::{IncrementalEngine, EngineError};
pub use node_id::NodeId;
pub use scheduler::UpdateReport;
pub use storage::MemoryStorage;
pub use transform::TransformError;
pub use slot::{SlotKind, SlotDescriptor, TransformSchema};
pub use collection::{CollectionDiff, ElementKey};