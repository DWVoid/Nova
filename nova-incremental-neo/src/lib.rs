//! `nova-incremental-neo` — next-generation incremental computation engine.
//!
//! ## Public API
//!
//! ```text
//! Engine, EngineBuilder, EngineError
//! UpdateReport
//! Storage, StorageError, StorageKey, StorageValue, MemoryStorage
//! Transform, TransformContext, TransformRegisterContext, TransformError
//! IncrementalValue, KeyExtractor, CollectionInput, CollectionOutputBuilder
//! TaskQueue, SequentialTaskQueue
//! Uuid
//! ```

mod engine;
mod keys;
mod report;
mod runner;
mod storage;
mod task_queue;
mod topology;
mod transform;
mod value_store;
mod workstate;

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

pub use engine::Engine;
pub use engine::EngineBuilder;
pub use engine::EngineError;

pub use report::UpdateReport;

pub use storage::Storage;
pub use storage::StorageError;
pub use storage::StorageKey;
pub use storage::StorageValue;
pub use storage::MemoryStorage;

pub use task_queue::TaskQueue;
pub use task_queue::SequentialTaskQueue;

pub use transform::Transform;
pub use transform::TransformContext;
pub use transform::TransformRegisterContext;
pub use transform::TransformError;
pub use transform::IncrementalValue;
pub use transform::KeyExtractor;
pub use transform::CollectionInput;
pub use transform::CollectionOutputBuilder;

pub use uuid::Uuid;
