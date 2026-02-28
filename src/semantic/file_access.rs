//! [`FileAccess`] – the user-supplied virtual file system trait.
//!
//! ## Design: Trait, Not Direct `std::fs` Calls
//!
//! The load transform reads file content through this trait rather than
//! calling `std::fs::read` directly.  This is deliberate:
//!
//! - **Testability**: tests supply an in-memory `MockFileAccess` with
//!   pre-loaded content; no real files or OS calls needed.
//! - **Virtual file systems**: an IDE can implement `FileAccess` backed by
//!   its in-memory document buffer so unsaved edits are visible to the
//!   compiler immediately.
//! - **Remote / embedded sources**: a build system might fetch files from a
//!   content-addressed store over the network.
//! - **Sandboxing**: the compiler never touches the real file system directly,
//!   which makes it easier to reason about access permissions.
//!
//! The trait is intentionally minimal – just one method.  Stat information
//! (size, mtime) flows in through [`FileStat`] inputs; the trait only needs
//! to provide the raw bytes.
//!
//! ## Design: `Arc<dyn FileAccess>` in the Transform
//!
//! The `LoadFile` transform holds an `Arc<dyn FileAccess>`.  This allows
//! multiple transforms (one per file) to share the same VFS instance cheaply.
//! The Arc also makes the transform `Clone`-able, which is required by the
//! incremental registry.
use std::sync::Arc;
use async_trait::async_trait;
/// Error returned when a file cannot be read.
#[derive(Debug, Clone)]
pub struct FileAccessError {
    pub path: String,
    pub message: String,
}
impl FileAccessError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { path: path.into(), message: message.into() }
    }
}
impl std::fmt::Display for FileAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot read '{}': {}", self.path, self.message)
    }
}
impl std::error::Error for FileAccessError {}
/// Async virtual file system used by the semantic pipeline to read source
/// file content.
///
/// Implement this trait and pass an `Arc<dyn FileAccess>` to
/// [`SemanticSession::new`] to provide the file content source.
///
/// # Example – real file system implementation
///
/// ```no_run
/// use async_trait::async_trait;
/// use nova::semantic::file_access::{FileAccess, FileAccessError};
///
/// pub struct RealFs;
///
/// #[async_trait]
/// impl FileAccess for RealFs {
///     async fn read_file(&self, path: &str) -> Result<Vec<u8>, FileAccessError> {
///         tokio::fs::read(path).await.map_err(|e| FileAccessError::new(path, e.to_string()))
///     }
/// }
/// ```
#[async_trait]
pub trait FileAccess: Send + Sync {
    /// Read the full byte content of the file at `path`.
    ///
    /// `path` is exactly the string stored in [`FileStat::path`].
    async fn read_file(&self, path: &str) -> Result<Vec<u8>, FileAccessError>;
}
// ---------------------------------------------------------------------------
// In-memory mock (used in tests)
// ---------------------------------------------------------------------------
/// An in-memory [`FileAccess`] implementation backed by a `HashMap`.
///
/// Pre-load files with [`MockFileAccess::add`] before running the session.
/// Returns [`FileAccessError`] for any path not in the map.
pub struct MockFileAccess {
    files: std::collections::HashMap<String, Vec<u8>>,
}
impl MockFileAccess {
    /// Create an empty mock.
    pub fn new() -> Self {
        Self { files: std::collections::HashMap::new() }
    }
    /// Register `content` under `path`.
    pub fn add(&mut self, path: impl Into<String>, content: impl Into<Vec<u8>>) {
        self.files.insert(path.into(), content.into());
    }
}
impl Default for MockFileAccess {
    fn default() -> Self { Self::new() }
}
#[async_trait]
impl FileAccess for MockFileAccess {
    async fn read_file(&self, path: &str) -> Result<Vec<u8>, FileAccessError> {
        self.files.get(path)
            .cloned()
            .ok_or_else(|| FileAccessError::new(path, "file not found in mock"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn mock_returns_registered_content() {
        let mut mock = MockFileAccess::new();
        mock.add("hello.nova", b"-- hello".to_vec());
        let bytes = mock.read_file("hello.nova").await.unwrap();
        assert_eq!(bytes, b"-- hello");
    }
    #[tokio::test]
    async fn mock_errors_for_missing_path() {
        let mock = MockFileAccess::new();
        let err = mock.read_file("missing.nova").await.unwrap_err();
        assert!(err.message.contains("not found"));
    }
}
