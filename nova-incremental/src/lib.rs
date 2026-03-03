//! `nova-incremental` — incremental computation engine for the Nova compiler.
//!
//! ## Public API
//!
//! ```text
//! Engine          — sealed runtime (update / get / checkpoint / commit / discard)
//! EngineBuilder   — static topology declaration
//! EngineError     — error type from engine operations
//! UpdateReport    — summary of one update() call
//! Storage         — trait: implement to supply a custom backend
//! StorageError    — error type from storage operations
//! MemoryStorage   — in-memory implementation (for tests / ephemeral use)
//! Transform       — trait: implement to define a computation step
//! TransformContext — per-invocation I/O (input() / output() / etc.)
//! TransformSchema — slot-layout builder (returned by Transform::schema())
//! TransformError  — error type from transform execution
//! IncrementalValue — auto-impl marker for types that flow through the graph
//! KeyExtractor    — derives stable u64 key from a collection element
//! CollectionInput — typed view of a gathered collection input
//! CollectionChange — incremental diff for a collection
//! Uuid            — re-exported for node identity
//! ```
//!
//! All internal modules are `pub(crate)` only.

mod engine;
mod graph;
mod loader;
mod node_id;
mod registry;
mod scheduler;
mod storage;
mod transform;
mod value;

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

// Engine lifecycle
pub use engine::Engine;
pub use engine::EngineBuilder;
pub use engine::EngineError;

// Update result
pub use scheduler::UpdateReport;

// Storage backend
pub use storage::Storage;
pub use storage::StorageError;
pub use storage::StorageKey;
pub use storage::StorageValue;
pub use storage::MemoryStorage;

// Transform authoring
pub use transform::Transform;
pub use transform::TransformContext;
pub use transform::TransformSchema;
pub use transform::TransformError;
pub use transform::IncrementalValue;
pub use transform::KeyExtractor;
pub use transform::CollectionInput;
pub use transform::CollectionChange;

// Stable node identity
pub use uuid::Uuid;
