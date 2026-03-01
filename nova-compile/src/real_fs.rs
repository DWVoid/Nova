//! Real file-system implementation of [`FileAccess`].
//!
//! ## Design: Thin Wrapper over `tokio::fs`
//!
//! The compiler driver reads source files directly from disk.  This module
//! provides the simplest possible [`FileAccess`] implementation: a unit
//! struct whose `read_file` method calls `tokio::fs::read`.
//!
//! The thin wrapper exists so that:
//! - The semantic pipeline never calls `std::fs` or `tokio::fs` directly;
//!   all I/O flows through the `FileAccess` abstraction.
//! - Tests in `nova-analyze` can swap in [`MockFileAccess`] without touching
//!   any driver code.
//!
//! ## Design: Paths Are Absolute
//!
//! The compiler driver canonicalises all source file paths before inserting
//! them into `FileStat` values (see `project::stat_source_files`).  By the
//! time `read_file` is called, `path` is always an absolute canonical path,
//! so no working-directory resolution is needed here.

use async_trait::async_trait;
use nova_analyze::semantic::file_access::{FileAccess, FileAccessError};

/// A [`FileAccess`] implementation that reads files from the real file system
/// using non-blocking I/O via `tokio::fs`.
///
/// Create a single instance and wrap it in `Arc` to share it across all
/// per-file load transforms:
///
/// ```no_run
/// use std::sync::Arc;
/// use nova_compile::real_fs::RealFileAccess;
///
/// let fs: Arc<dyn nova_analyze::semantic::file_access::FileAccess> =
///     Arc::new(RealFileAccess);
/// ```
pub struct RealFileAccess;

#[async_trait]
impl FileAccess for RealFileAccess {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>, FileAccessError> {
        tokio::fs::read(path).await.map_err(|e| {
            FileAccessError::new(path, e.to_string())
        })
    }
}
