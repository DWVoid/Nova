//! redb-based storage backend for the incremental engine.
//!
//! ## Tables
//!
//! | Table | Key | Value | Purpose |
//! |-------|-----|-------|---------|
//! | `main` | `[u8; 16]` (UUID) | `Vec<u8>` | Committed data |
//! | `staging` | `[u8; 16]` (UUID) | `Vec<u8>` | Checkpoint staging (empty = tombstone) |
//!
//! ## Design: Checkpoint / Commit / Discard
//!
//! - [`checkpoint`] — activates staging mode.  Subsequent `set`/`delete` calls
//!   write to the `staging` table instead of `main`.  Deletes write an empty
//!   tombstone value to staging.
//!
//! - [`get`] — when staging is active, checks the `staging` table first.
//!   An empty value (tombstone) is treated as "not found".
//!   Falls through to `main` if absent from staging.
//!
//! - [`commit`] — iterates every entry in `staging`, writes each to `main`
//!   (or removes from `main` if the value is empty/tombstone), then clears
//!   the `staging` table.
//!
//! - [`discard`] — clears the `staging` table, losing all in-flight changes.
//!
//! ## Async safety
//!
//! All redb operations are synchronous and wrapped in
//! [`tokio::task::spawn_blocking`] so the async runtime is never blocked.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use nova_incremental::{Storage, StorageKey, StorageValue, StorageError};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Committed data table.
const MAIN_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("main");

/// Checkpoint staging table.
const STAGING_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("staging");

/// Value recorded in the staging table to mark a key as deleted.
const TOMBSTONE: &[u8] = b"";

/// Database file name created inside the storage directory.
const DB_FILENAME: &str = "storage.redb";

// ---------------------------------------------------------------------------
// Error conversion helpers
// ---------------------------------------------------------------------------

fn storage_err(msg: impl std::fmt::Display) -> StorageError {
    StorageError::new(msg.to_string())
}

fn storage_err_with(
    msg: impl std::fmt::Display,
    src: impl std::fmt::Display,
) -> StorageError {
    StorageError::with_source(msg.to_string(), src.to_string())
}

// ---------------------------------------------------------------------------
// RedbStorage
// ---------------------------------------------------------------------------

/// A [`Storage`] backend backed by a single redb database file.
pub struct RedbStorage {
    db: Arc<Database>,
    checkpoint_active: AtomicBool,
}

impl RedbStorage {
    /// Open (or create) the redb database at `dir / "storage.redb"`.
    pub fn new(dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).map_err(|e| {
            storage_err_with("cannot create storage dir", e)
        })?;

        let db_path = dir.join(DB_FILENAME);
        let db = Database::create(&db_path).map_err(|e| {
            storage_err_with("cannot create redb database", e)
        })?;

        // Ensure both tables exist for readers.
        let txn = db.begin_write().map_err(|e| {
            storage_err_with("begin_write for init", e)
        })?;
        {
            let _ = txn.open_table(MAIN_TABLE);
            let _ = txn.open_table(STAGING_TABLE);
        }
        txn.commit().map_err(|e| storage_err_with("commit init tables", e))?;

        Ok(Self {
            db: Arc::new(db),
            checkpoint_active: AtomicBool::new(false),
        })
    }

    fn key_bytes(key: &StorageKey) -> [u8; 16] {
        *key.as_uuid().as_bytes()
    }
}

// ---------------------------------------------------------------------------
// Storage trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Storage for RedbStorage {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError> {
        let kb = Self::key_bytes(key);
        let is_cp = self.checkpoint_active.load(Ordering::Relaxed);
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || -> Result<Option<StorageValue>, StorageError> {
            if is_cp {
                // Check staging first.
                let rt: redb::ReadTransaction =
                    db.begin_read().map_err(|e| storage_err_with("begin_read", e))?;
                if let Ok(rtable) = rt.open_table(STAGING_TABLE) {
                    if let Some(guard) = rtable
                        .get(&kb[..])
                        .map_err(|e| storage_err_with("staging get", e))?
                    {
                        let val = guard.value();
                        if val.is_empty() {
                            return Ok(None); // tombstone
                        }
                        return Ok(Some(StorageValue::new(val.to_vec())));
                    }
                }
            }

            // Fall back to main.
            let rt: redb::ReadTransaction =
                db.begin_read().map_err(|e| storage_err_with("begin_read", e))?;
            if let Ok(rtable) = rt.open_table(MAIN_TABLE) {
                if let Some(guard) = rtable
                    .get(&kb[..])
                    .map_err(|e| storage_err_with("main get", e))?
                {
                    return Ok(Some(StorageValue::new(guard.value().to_vec())));
                }
            }
            Ok(None)
        })
        .await
        .map_err(|e| storage_err(format!("spawn_blocking: {e}")))?
    }

    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError> {
        let kb = Self::key_bytes(key);
        let is_cp = self.checkpoint_active.load(Ordering::Relaxed);
        let table_def = if is_cp { STAGING_TABLE } else { MAIN_TABLE };
        let val = value.as_bytes().to_vec();
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let wt: redb::WriteTransaction =
                db.begin_write().map_err(|e| storage_err_with("begin_write", e))?;
            {
                let mut table: redb::Table<'_, &[u8], &[u8]> =
                    wt.open_table(table_def)
                        .map_err(|e| storage_err_with("open_table", e))?;
                table
                    .insert(&kb[..], &val[..])
                    .map_err(|e| storage_err_with("insert", e))?;
            }
            wt.commit().map_err(|e| storage_err_with("commit", e))?;
            Ok(())
        })
        .await
        .map_err(|e| storage_err(format!("spawn_blocking: {e}")))?
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        let kb = Self::key_bytes(key);
        let is_cp = self.checkpoint_active.load(Ordering::Relaxed);
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let wt: redb::WriteTransaction =
                db.begin_write().map_err(|e| storage_err_with("begin_write", e))?;
            if is_cp {
                let mut table: redb::Table<'_, &[u8], &[u8]> =
                    wt.open_table(STAGING_TABLE)
                        .map_err(|e| storage_err_with("open staging", e))?;
                table
                    .insert(&kb[..], TOMBSTONE)
                    .map_err(|e| storage_err_with("insert tombstone", e))?;
            } else {
                let mut table: redb::Table<'_, &[u8], &[u8]> =
                    wt.open_table(MAIN_TABLE)
                        .map_err(|e| storage_err_with("open main", e))?;
                let _ = table.remove(&kb[..]);
            }
            wt.commit().map_err(|e| storage_err_with("commit", e))?;
            Ok(())
        })
        .await
        .map_err(|e| storage_err(format!("spawn_blocking: {e}")))?
    }

    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError> {
        Ok(self.get(key).await?.is_some())
    }

    async fn checkpoint(&self) -> Result<(), StorageError> {
        if self.checkpoint_active.swap(true, Ordering::Relaxed) {
            return Err(storage_err("checkpoint already active"));
        }
        Ok(())
    }

    async fn commit(&self) -> Result<(), StorageError> {
        if !self.checkpoint_active.load(Ordering::Relaxed) {
            return Err(storage_err("no active checkpoint to commit"));
        }

        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let wt: redb::WriteTransaction =
                db.begin_write().map_err(|e| storage_err_with("begin_write", e))?;

            // Drain staging table and apply to main.
            {
                let st: redb::Table<'_, &[u8], &[u8]> =
                    wt.open_table(STAGING_TABLE)
                        .map_err(|e| storage_err_with("open staging", e))?;
                let mut mt: redb::Table<'_, &[u8], &[u8]> =
                    wt.open_table(MAIN_TABLE)
                        .map_err(|e| storage_err_with("open main", e))?;

                for item in st
                    .iter()
                    .map_err(|e| storage_err_with("iterate staging", e))?
                {
                    let (k_guard, v_guard) =
                        item.map_err(|e| storage_err_with("read staging entry", e))?;
                    let key_data = k_guard.value().to_vec();
                    let val_data = v_guard.value().to_vec();
                    if val_data.is_empty() {
                        let _ = mt.remove(key_data.as_slice());
                    } else {
                        mt.insert(key_data.as_slice(), val_data.as_slice())
                            .map_err(|e| storage_err_with("apply to main", e))?;
                    }
                }
            }

            // Reset staging for next checkpoint cycle.
            wt.delete_table(STAGING_TABLE)
                .map_err(|e| storage_err_with("delete staging", e))?;
            wt.open_table(STAGING_TABLE)
                .map_err(|e| storage_err_with("re-create staging", e))?;

            wt.commit().map_err(|e| storage_err_with("commit checkpoint", e))?;
            Ok(())
        })
        .await
        .map_err(|e| storage_err(format!("spawn_blocking: {e}")))??;

        self.checkpoint_active.store(false, Ordering::Relaxed);
        Ok(())
    }

    async fn discard(&self) -> Result<(), StorageError> {
        if !self.checkpoint_active.load(Ordering::Relaxed) {
            return Err(storage_err("no active checkpoint to discard"));
        }

        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
            let wt: redb::WriteTransaction =
                db.begin_write().map_err(|e| storage_err_with("begin_write", e))?;

            wt.delete_table(STAGING_TABLE)
                .map_err(|e| storage_err_with("delete staging", e))?;
            wt.open_table(STAGING_TABLE)
                .map_err(|e| storage_err_with("re-create staging", e))?;

            wt.commit().map_err(|e| storage_err_with("commit discard", e))?;
            Ok(())
        })
        .await
        .map_err(|e| storage_err(format!("spawn_blocking: {e}")))??;

        self.checkpoint_active.store(false, Ordering::Relaxed);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use nova_incremental::Uuid;

    fn new_key() -> StorageKey {
        StorageKey::from_uuid(Uuid::new_v4())
    }

    fn make_store() -> (RedbStorage, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = RedbStorage::new(tmp.path()).unwrap();
        (store, tmp)
    }

    #[tokio::test]
    async fn set_and_get_roundtrip() {
        let (store, _tmp) = make_store();
        let key = new_key();
        let val = StorageValue::new(b"hello redb".to_vec());
        store.set(&key, val.clone()).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), Some(val));
    }

    #[tokio::test]
    async fn missing_key_returns_none() {
        let (store, _tmp) = make_store();
        assert_eq!(store.get(&new_key()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let (store, _tmp) = make_store();
        let key = new_key();
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        assert!(store.contains(&key).await.unwrap());
        store.delete(&key).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn overwrite_value() {
        let (store, _tmp) = make_store();
        let key = new_key();
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.set(&key, StorageValue::new(vec![2, 3])).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap().unwrap().as_bytes(), &[2, 3]);
    }

    #[tokio::test]
    async fn checkpoint_commit_persists() {
        let (store, _tmp) = make_store();
        let key = new_key();
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.checkpoint().await.unwrap();
        store.set(&key, StorageValue::new(vec![99])).await.unwrap();
        store.commit().await.unwrap();
        assert_eq!(store.get(&key).await.unwrap().unwrap().as_bytes(), &[99]);
    }

    #[tokio::test]
    async fn checkpoint_discard_rolls_back() {
        let (store, _tmp) = make_store();
        let key = new_key();
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.checkpoint().await.unwrap();
        store.set(&key, StorageValue::new(vec![2])).await.unwrap();
        store.discard().await.unwrap();
        assert_eq!(store.get(&key).await.unwrap().unwrap().as_bytes(), &[1]);
    }

    #[tokio::test]
    async fn checkpoint_commit_delete() {
        let (store, _tmp) = make_store();
        let key = new_key();
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.checkpoint().await.unwrap();
        store.delete(&key).await.unwrap();
        store.commit().await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn checkpoint_discard_reverts_delete() {
        let (store, _tmp) = make_store();
        let key = new_key();
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.checkpoint().await.unwrap();
        store.delete(&key).await.unwrap();
        store.discard().await.unwrap();
        assert_eq!(store.get(&key).await.unwrap().unwrap().as_bytes(), &[1]);
    }

    #[tokio::test]
    async fn staging_read_visibility() {
        let (store, _tmp) = make_store();
        let key = new_key();
        store.set(&key, StorageValue::new(vec![10])).await.unwrap();
        store.checkpoint().await.unwrap();
        store.set(&key, StorageValue::new(vec![20])).await.unwrap();
        assert_eq!(store.get(&key).await.unwrap().unwrap().as_bytes(), &[20]);
        store.discard().await.unwrap();
        assert_eq!(store.get(&key).await.unwrap().unwrap().as_bytes(), &[10]);
    }

    #[tokio::test]
    async fn multiple_keys() {
        let (store, _tmp) = make_store();
        let k1 = new_key();
        let k2 = new_key();
        store.set(&k1, StorageValue::new(b"a".to_vec())).await.unwrap();
        store.set(&k2, StorageValue::new(b"b".to_vec())).await.unwrap();
        assert_eq!(store.get(&k1).await.unwrap().unwrap().as_bytes(), b"a");
        assert_eq!(store.get(&k2).await.unwrap().unwrap().as_bytes(), b"b");
    }
}
