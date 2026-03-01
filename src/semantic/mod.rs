//! # Semantic Analysis Pipeline
//!
//! This module implements the **file-loading stage** of the Nova semantic
//! pipeline, built on top of the [`crate::incremental`] computation graph.
//!
//! ## Overview
//!
//! ```text
//!  User / IDE
//!     │  supplies FileStat list (path, size, mtime)
//!     │  supplies Arc<dyn Storage>  (incremental persistence)
//!     │  supplies Arc<dyn FileAccess> (VFS)
//!     ▼
//!  SemanticSession               (session.rs)
//!     │
//!     │  per file:
//!     │  ┌─────────────────────────────────────────────────────────┐
//!     │  │  [stat_node: FileStat]  ──LoadFile──>  [content_node: FileContent] │
//!     │  └─────────────────────────────────────────────────────────┘
//!     │
//!     └─ IncrementalEngine (incremental::engine)
//!             └─ Arc<dyn Storage>
//! ```
//!
//! ## Stages (this module)
//!
//! | Stage | Input | Output | File |
//! |-------|-------|--------|------|
//! | File stat input | external scan | `FileStat` | `file_stat.rs` |
//! | File load | `FileStat` | `FileContent` | `load_transform.rs` |
//!
//! ## Usage
//!
//! ```no_run
//! use std::sync::Arc;
//! use nova::incremental::storage::MemoryStorage;
//! use nova::semantic::{SemanticSession, FileStat};
//! use nova::semantic::file_access::MockFileAccess;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut mock = MockFileAccess::new();
//!     mock.add("main.nova", b"val x = 1;".to_vec());
//!
//!     let storage = Arc::new(MemoryStorage::new());
//!     let fs      = Arc::new(mock);
//!
//!     let mut session = SemanticSession::new(storage, fs);
//!     session.update_files(vec![FileStat::new("main.nova", 10, 0)]).unwrap();
//!     session.run().await;
//!
//!     let content = session.get_content("main.nova").await.unwrap().unwrap();
//!     println!("{}", content.as_str().unwrap());
//! }
//! ```

pub mod file_access;
pub mod file_content;
pub mod file_stat;
pub mod load_transform;
pub mod lex_transform;
pub mod parse_transform;
pub mod session;

// Top-level re-exports.
pub use file_stat::FileStat;
pub use file_content::FileContent;
pub use file_access::{FileAccess, FileAccessError, MockFileAccess};
pub use lex_transform::LexFile;
pub use parse_transform::ParseFile;
pub use session::{SemanticSession, FileNodes, stat_node_id, content_node_id, lex_node_id, parse_node_id};
