//! Loose-file storage backend for the incremental engine.
//!
//! ## Design: One File Per UUID Key
//!
//! Each [`StorageKey`] (which is a UUID) maps to a single file named
//! `<uuid-simple>.bin` under the storage directory.  UUID hex strings are
//! already safe file-name characters, so no encoding is needed.
//!
//! ## Design: Atomic Writes
//!
//! Each `set` writes a `.tmp` sibling then renames it over the target so a
//! crash between write and rename never produces a corrupt file.
//!
//! ## Design: Checkpoint / Commit / Discard
//!
//! When a checkpoint is active, `set` and `delete` operate on staging files
//! (suffixed `.staged`).  On `commit`, each staged file is renamed over its
//! target.  On `discard`, staged files are simply deleted.
//!
//! ## TODO
//!
//! Replace with an embedded key-value store (`redb`, `sled`, …) once the
//! project's storage requirements are better understood.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use nova_incremental_neo::{Storage, StorageKey, StorageValue, StorageError};

// ---------------------------------------------------------------------------
// FileSystemStorage
// ---------------------------------------------------------------------------

pub struct FileSystemStorage {
    dir: PathBuf,
    /// Tracks keys written/deleted while a checkpoint is active.
    staged_writes:  Arc<Mutex<Option<HashSet<String>>>>,
    staged_deletes: Arc<Mutex<Option<HashSet<String>>>>,
}

impl FileSystemStorage {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            staged_writes:  Arc::new(Mutex::new(None)),
            staged_deletes: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn ensure_dir(&self) -> Result<(), StorageError> {
        tokio::fs::create_dir_all(&self.dir).await.map_err(|e| {
            StorageError::with_source(
                format!("cannot create storage dir '{}'", self.dir.display()),
                e.to_string(),
            )
        })
    }

    fn key_to_filename(key: &StorageKey) -> String {
        format!("{}.bin", key.as_uuid().simple())
    }

    fn staged_filename(key: &StorageKey) -> String {
        format!("{}.staged", key.as_uuid().simple())
    }

    fn target_path(&self, key: &StorageKey) -> PathBuf {
        self.dir.join(Self::key_to_filename(key))
    }

    fn staged_path(&self, key: &StorageKey) -> PathBuf {
        self.dir.join(Self::staged_filename(key))
    }

    fn is_checkpoint_active(&self) -> bool {
        self.staged_writes.lock().unwrap().is_some()
    }
}

#[async_trait]
impl Storage for FileSystemStorage {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError> {
        // Resolve staged state without holding the lock across awaits.
        let staged_fname    = Self::staged_filename(key);
        let committed_fname = Self::key_to_filename(key);

        let has_staged_write = {
            let sw = self.staged_writes.lock().unwrap();
            sw.as_ref().map(|set| set.contains(&staged_fname)).unwrap_or(false)
        };
        if has_staged_write {
            let path = self.staged_path(key);
            match tokio::fs::read(&path).await {
                Ok(bytes) => return Ok(Some(StorageValue::new(bytes))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(StorageError::with_source(
                    format!("read staged '{}'", path.display()), e.to_string())),
            }
        }

        let is_staged_delete = {
            let sd = self.staged_deletes.lock().unwrap();
            sd.as_ref().map(|set| set.contains(&committed_fname)).unwrap_or(false)
        };
        if is_staged_delete { return Ok(None); }

        let path = self.target_path(key);
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(Some(StorageValue::new(bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::with_source(
                format!("read '{}'", path.display()), e.to_string())),
        }
    }

    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError> {
        let is_cp = self.is_checkpoint_active();
        let target = if is_cp { self.staged_path(key) } else { self.target_path(key) };
        let tmp = target.with_extension("tmp");
        tokio::fs::write(&tmp, value.as_bytes()).await.map_err(|e| {
            StorageError::with_source(format!("write tmp '{}'", tmp.display()), e.to_string())
        })?;
        tokio::fs::rename(&tmp, &target).await.map_err(|e| {
            StorageError::with_source(
                format!("rename '{}' → '{}'", tmp.display(), target.display()), e.to_string())
        })?;
        if is_cp {
            let staged_fname = Self::staged_filename(key);
            self.staged_writes.lock().unwrap().as_mut().unwrap().insert(staged_fname);
        }
        Ok(())
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        let is_cp = self.is_checkpoint_active();
        if is_cp {
            self.staged_deletes.lock().unwrap().as_mut().unwrap().insert(Self::key_to_filename(key));
            return Ok(());
        }
        let path = self.target_path(key);
        match tokio::fs::remove_file(&path).await {
            Ok(()) | Err(_) => Ok(()),
        }
    }

    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError> {
        match self.get(key).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    async fn checkpoint(&self) -> Result<(), StorageError> {
        let mut sw = self.staged_writes.lock().unwrap();
        if sw.is_some() {
            return Err(StorageError::new("checkpoint already active"));
        }
        *sw = Some(HashSet::new());
        *self.staged_deletes.lock().unwrap() = Some(HashSet::new());
        Ok(())
    }

    async fn commit(&self) -> Result<(), StorageError> {
        // Apply staged writes: rename .staged → .bin
        let staged: HashSet<String> = {
            let mut sw = self.staged_writes.lock().unwrap();
            sw.take().ok_or_else(|| StorageError::new("no active checkpoint to commit"))?
        };
        for fname in &staged {
            let staged_path = self.dir.join(fname);
            let target_name = fname.replace(".staged", ".bin");
            let target_path = self.dir.join(&target_name);
            if let Err(e) = tokio::fs::rename(&staged_path, &target_path).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(StorageError::with_source(
                        format!("commit: rename '{}'→'{}'", staged_path.display(), target_path.display()),
                        e.to_string(),
                    ));
                }
            }
        }
        // Apply staged deletes
        let deletes = self.staged_deletes.lock().unwrap().take().unwrap_or_default();
        for fname in &deletes {
            let path = self.dir.join(fname);
            let _ = tokio::fs::remove_file(&path).await;
        }
        Ok(())
    }

    async fn discard(&self) -> Result<(), StorageError> {
        let staged = {
            let mut sw = self.staged_writes.lock().unwrap();
            sw.take().ok_or_else(|| StorageError::new("no active checkpoint to discard"))?
        };
        *self.staged_deletes.lock().unwrap() = None;
        // Remove staged files.
        for fname in &staged {
            let path = self.dir.join(fname);
            let _ = tokio::fs::remove_file(&path).await;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use nova_incremental_neo::Uuid;

    fn new_key() -> StorageKey { StorageKey::from_uuid(Uuid::new_v4()) }

    async fn make_store() -> (FileSystemStorage, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = FileSystemStorage::new(tmp.path());
        store.ensure_dir().await.unwrap();
        (store, tmp)
    }

    #[tokio::test]
    async fn set_and_get_roundtrip() {
        let (store, _tmp) = make_store().await;
        let key = new_key();
        let val = StorageValue::new(b"world".to_vec());
        store.set(&key, val.clone()).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), Some(val));
    }

    #[tokio::test]
    async fn missing_key_returns_none() {
        let (store, _tmp) = make_store().await;
        let key = new_key();
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn contains_before_and_after_set() {
        let (store, _tmp) = make_store().await;
        let key = new_key();
        assert!(!store.contains(&key).await.unwrap());
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        assert!(store.contains(&key).await.unwrap());
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let (store, _tmp) = make_store().await;
        let key = new_key();
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.delete(&key).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn checkpoint_commit_persists() {
        let (store, _tmp) = make_store().await;
        let key = new_key();
        store.checkpoint().await.unwrap();
        store.set(&key, StorageValue::new(vec![42])).await.unwrap();
        store.commit().await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), Some(StorageValue::new(vec![42])));
    }

    #[tokio::test]
    async fn checkpoint_discard_rolls_back() {
        let (store, _tmp) = make_store().await;
        let key = new_key();
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.checkpoint().await.unwrap();
        store.set(&key, StorageValue::new(vec![2])).await.unwrap();
        store.discard().await.unwrap();
        // The staged file should be gone; the committed file should be as it was.
        assert_eq!(store.get(&key).await.unwrap(), Some(StorageValue::new(vec![1])));
    }
}