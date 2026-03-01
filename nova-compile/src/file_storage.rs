//! Loose-file storage backend for the incremental engine.
//!
//! ## Design: One File Per Key
//!
//! Before a proper embedded key-value store is integrated, this backend
//! stores each key as a single file on disk.  The mapping is:
//!
//! ```text
//! <storage_dir>/<percent-encoded-key>.bin
//! ```
//!
//! Percent-encoding is used so that keys containing `/`, `\`, `:`, and
//! other characters that are invalid in file names on various platforms are
//! safely represented on disk without any collision risk.
//!
//! ## Design: `tokio::fs` for Async I/O
//!
//! All disk operations use `tokio::fs` so that they do not block the async
//! executor.  This matches the `async_trait` signature required by
//! `nova_incremental::storage::Storage`.
//!
//! ## Design: Atomic Writes
//!
//! Each `set` operation writes a temporary sibling file and then renames it
//! over the target.  On most POSIX file systems, `rename` is an atomic
//! operation, so a crash between `write_all` and `rename` leaves the old
//! value intact rather than producing a truncated or corrupt file.
//!
//! ## Design: Transitional
//!
//! This backend is intentionally simple and carries a clear "TODO: replace"
//! note.  Loose files have a linear O(n) directory scan cost for `list`
//! operations and are limited by the file system's inode table.  A future
//! version should replace this with `redb`, `rocksdb`, or `sled`.

use std::path::{Path, PathBuf};
use async_trait::async_trait;
use nova_incremental::storage::{Storage, StorageKey, StorageValue, StorageError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Percent-encode a key string so it is safe to use as a file name.
///
/// Only alphanumerics, `-`, `_`, and `.` are left unencoded.  Everything
/// else is replaced by `%HH` where `HH` is the uppercase hexadecimal byte
/// value.
fn encode_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len() * 3);
    for byte in key.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(byte as char);
            }
            other => {
                out.push('%');
                out.push_str(&format!("{:02X}", other));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// FileSystemStorage
// ---------------------------------------------------------------------------

/// A [`Storage`] implementation that stores each key as a loose file on disk.
///
/// Create one with [`FileSystemStorage::new`] and pass the directory under
/// which all files should be kept.  The directory is created on demand by
/// [`FileSystemStorage::ensure_dir`].
///
/// # Persistence layout
///
/// ```text
/// <project>/target/nova-incremental/
///     __graph_meta__.bin          ← graph topology
///     6ba7b814-....bin            ← node value (UUID-derived key)
///     6ba7b814-....bin.tmp        ← temporary during atomic write (may linger after crash)
///     ...
/// ```
///
/// # TODO
///
/// Replace with an embedded key-value store such as `redb` or `sled` once
/// the project's storage requirements are better understood.
pub struct FileSystemStorage {
    /// Absolute path to the directory used as the key-value store.
    dir: PathBuf,
}

impl FileSystemStorage {
    /// Create a new storage bound to `dir`.
    ///
    /// The directory is **not** created here; call [`ensure_dir`](Self::ensure_dir)
    /// before the first write to make sure it exists.
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self { dir: dir.as_ref().to_path_buf() }
    }

    /// Create the storage directory (and all ancestors) if it does not exist.
    pub async fn ensure_dir(&self) -> Result<(), StorageError> {
        tokio::fs::create_dir_all(&self.dir).await.map_err(|e| {
            StorageError::with_source(
                format!("cannot create storage dir '{}'", self.dir.display()),
                e.to_string(),
            )
        })
    }

    /// Map a [`StorageKey`] to the path of the corresponding loose file.
    fn key_path(&self, key: &StorageKey) -> PathBuf {
        self.dir.join(format!("{}.bin", encode_key(key.as_str())))
    }
}

#[async_trait]
impl Storage for FileSystemStorage {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError> {
        let path = self.key_path(key);
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(Some(StorageValue::new(bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::with_source(
                format!("read '{}'", path.display()),
                e.to_string(),
            )),
        }
    }

    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError> {
        let target = self.key_path(key);

        // Write to a temporary sibling file, then atomically rename.
        let tmp = target.with_extension("bin.tmp");
        tokio::fs::write(&tmp, value.as_bytes()).await.map_err(|e| {
            StorageError::with_source(
                format!("write tmp '{}'", tmp.display()),
                e.to_string(),
            )
        })?;
        tokio::fs::rename(&tmp, &target).await.map_err(|e| {
            StorageError::with_source(
                format!("rename '{}' -> '{}'", tmp.display(), target.display()),
                e.to_string(),
            )
        })?;
        Ok(())
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        let path = self.key_path(key);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::with_source(
                format!("delete '{}'", path.display()),
                e.to_string(),
            )),
        }
    }

    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError> {
        let path = self.key_path(key);
        match tokio::fs::metadata(&path).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(StorageError::with_source(
                format!("stat '{}'", path.display()),
                e.to_string(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a storage instance rooted in a fresh temp directory.
    async fn make_store() -> (FileSystemStorage, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = FileSystemStorage::new(tmp.path());
        store.ensure_dir().await.unwrap();
        (store, tmp)
    }

    #[tokio::test]
    async fn set_and_get_roundtrip() {
        let (store, _tmp) = make_store().await;
        let key = StorageKey::new("hello");
        let val = StorageValue::new(b"world".to_vec());
        store.set(&key, val.clone()).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), Some(val));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (store, _tmp) = make_store().await;
        assert_eq!(store.get(&StorageKey::new("absent")).await.unwrap(), None);
    }

    #[tokio::test]
    async fn contains_reflects_presence() {
        let (store, _tmp) = make_store().await;
        let key = StorageKey::new("check");
        assert!(!store.contains(&key).await.unwrap());
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        assert!(store.contains(&key).await.unwrap());
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let (store, _tmp) = make_store().await;
        let key = StorageKey::new("del-me");
        store.set(&key, StorageValue::new(vec![42])).await.unwrap();
        store.delete(&key).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn delete_missing_is_ok() {
        let (store, _tmp) = make_store().await;
        store.delete(&StorageKey::new("ghost")).await.unwrap();
    }

    #[tokio::test]
    async fn key_with_special_characters() {
        let (store, _tmp) = make_store().await;
        let key = StorageKey::new("path/to/file:version?q=1&r=2");
        let val = StorageValue::new(b"data".to_vec());
        store.set(&key, val.clone()).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), Some(val));
    }

    #[test]
    fn encode_key_safe_chars_unchanged() {
        assert_eq!(encode_key("abc-_123.bin"), "abc-_123.bin");
    }

    #[test]
    fn encode_key_encodes_slash() {
        assert_eq!(encode_key("a/b"), "a%2Fb");
    }

    #[test]
    fn encode_key_encodes_colon() {
        assert_eq!(encode_key("a:b"), "a%3Ab");
    }
}
