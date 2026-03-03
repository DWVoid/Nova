//! `nova-incremental` — incremental computation engine for the Nova compiler.
//!
//! ## Public API (unchanged from user perspective)
//!
//! ```text
//! Engine, EngineBuilder, EngineError
//! UpdateReport
//! Storage, StorageError, StorageKey, StorageValue, MemoryStorage
//! Transform, TransformContext, TransformRegisterContext, TransformError
//! IncrementalValue, KeyExtractor, CollectionInput, CollectionChange
//! Uuid
//! ```

mod engine;
mod loader;
mod node_id;
mod scheduler;
mod storage;
mod topology;
mod transform;
mod value;
mod workstate;

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
pub use transform::TransformRegisterContext;
pub use transform::TransformError;
pub use transform::IncrementalValue;
pub use transform::KeyExtractor;
pub use transform::CollectionInput;
pub use transform::CollectionChange;

// Stable node identity
pub use uuid::Uuid;