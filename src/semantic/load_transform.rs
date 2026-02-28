//! [`LoadFile`] – the incremental transform that reads a source file.
//!
//! ## Design: One Transform Instance Per File
//!
//! Each source file gets its own `LoadFile` transform node in the graph.
//! The transform is keyed in the [`TransformRegistry`] under the path string
//! (prefixed with `"load:"`) so that it can be restored from persistence.
//!
//! An alternative design would be a single "load dispatcher" transform that
//! reads the path from the input stat and dispatches to the VFS.  We reject
//! this because:
//! - It couples all file loads into one node, preventing parallel execution.
//! - A single error would poison all file loads instead of just one.
//! - Per-file nodes give independent dirty flags, hash caches, and error
//!   states.
//!
//! ## Design: Path Captured at Construction, Not Read from Input
//!
//! `LoadFile` captures the `path` string at construction time.  It receives
//! the `FileStat` as its input value only to participate in the dirty-flag
//! and hash-based early-exit machinery.  If the stat hash is unchanged the
//! load is skipped entirely without ever calling `FileAccess::read_file`.
//!
//! The path is re-derived from the stat at runtime as a sanity check (the
//! stat's path must match the captured path).

use std::sync::Arc;
use async_trait::async_trait;

use crate::incremental::{
    value::Value,
    transform::{OneToOneTransform, TransformError},
};
use crate::semantic::file_access::FileAccess;
use crate::semantic::file_stat::FileStat;
use crate::semantic::file_content::FileContent;

/// A 1→1 incremental transform that loads a single source file.
///
/// **Input**: a [`Value`] wrapping a [`FileStat`].  
/// **Output**: a [`Value`] wrapping a [`FileContent`].
///
/// The transform is constructed with an `Arc<dyn FileAccess>` that it calls
/// to fetch the bytes.  Because the transform is registered in the engine's
/// [`TransformRegistry`] under a per-file key, a single `Arc<dyn FileAccess>`
/// is shared across all per-file load transforms cheaply.
pub struct LoadFile {
    /// Captured path – must match `stat.path` at runtime.
    pub path: String,
    /// Shared VFS instance provided by the user.
    pub fs: Arc<dyn FileAccess>,
}

impl LoadFile {
    /// Create a transform for the file at `path` using the given `FileAccess`.
    pub fn new(path: impl Into<String>, fs: Arc<dyn FileAccess>) -> Self {
        Self { path: path.into(), fs }
    }

    /// Return the registry key used for this file's load transform.
    ///
    /// The key is `"load:<path>"`.  It is stable as long as the path is
    /// stable, which is required for persistence across restarts.
    pub fn registry_key(path: &str) -> String {
        format!("load:{path}")
    }
}

#[async_trait]
impl OneToOneTransform for LoadFile {
    /// Load the file and return its content.
    ///
    /// # Errors
    ///
    /// - If the input value is not a [`FileStat`], returns a type-mismatch error.
    /// - If the stat path does not match the transform's captured path, returns
    ///   a consistency error.
    /// - If [`FileAccess::read_file`] fails, wraps the error as a
    ///   [`TransformError`].
    async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
        // Downcast to FileStat.
        let stat = input.downcast::<FileStat>().ok_or_else(|| {
            TransformError::new(format!(
                "LoadFile({}): input is not a FileStat", self.path
            ))
        })?;

        // Sanity-check that the stat refers to the same file this transform
        // was built for.  A mismatch would indicate a wiring bug.
        if stat.path != self.path {
            return Err(TransformError::new(format!(
                "LoadFile({}): stat path mismatch – got '{}'",
                self.path, stat.path
            )));
        }

        // Delegate to the VFS.
        let bytes = self.fs.read_file(&self.path).await.map_err(|e| {
            TransformError::with_source(
                format!("LoadFile({}): read failed", self.path),
                e.to_string(),
            )
        })?;

        Ok(Value::new(FileContent::new(self.path.clone(), bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::file_access::MockFileAccess;
    use crate::incremental::transform::OneToOneTransform;

    fn make_fs(path: &str, content: &[u8]) -> Arc<dyn FileAccess> {
        let mut mock = MockFileAccess::new();
        mock.add(path, content.to_vec());
        Arc::new(mock)
    }

    #[tokio::test]
    async fn loads_file_content() {
        let fs = make_fs("src/main.nova", b"val x = 1;");
        let t  = LoadFile::new("src/main.nova", fs);
        let stat = FileStat::new("src/main.nova", 10, 0);
        let out = t.apply(&Value::new(stat)).await.unwrap();
        let content = out.downcast::<FileContent>().unwrap();
        assert_eq!(content.bytes, b"val x = 1;");
        assert_eq!(content.path, "src/main.nova");
    }

    #[tokio::test]
    async fn wrong_input_type_is_error() {
        let fs = make_fs("f.nova", b"x");
        let t  = LoadFile::new("f.nova", fs);
        let result = t.apply(&Value::new(42i32)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("not a FileStat"));
    }

    #[tokio::test]
    async fn path_mismatch_is_error() {
        let fs = make_fs("a.nova", b"x");
        let t  = LoadFile::new("a.nova", fs);
        let stat = FileStat::new("b.nova", 1, 0); // different path
        let result = t.apply(&Value::new(stat)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("mismatch"));
    }

    #[tokio::test]
    async fn missing_file_is_error() {
        let fs = Arc::new(MockFileAccess::new()); // empty FS
        let t  = LoadFile::new("missing.nova", fs);
        let stat = FileStat::new("missing.nova", 0, 0);
        let result = t.apply(&Value::new(stat)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("read failed"));
    }

    #[test]
    fn registry_key_format() {
        assert_eq!(LoadFile::registry_key("src/foo.nova"), "load:src/foo.nova");
    }
}
