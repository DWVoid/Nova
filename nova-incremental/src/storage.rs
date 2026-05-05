//! Async key-value storage trait + [`MemoryStorage`] in-memory backend.
//!
//! ## Public surface
//! - [`Storage`] — trait to implement for custom backends.
//! - [`StorageError`] — error type for storage operations.
//! - [`StorageKey`] — opaque UUID-based key (usable by storage implementors).
//! - [`StorageValue`] — raw bytes wrapper (usable by storage implementors).
//! - [`MemoryStorage`] — built-in in-memory backend.

use async_trait::async_trait;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// StorageError
// ---------------------------------------------------------------------------

/// Error returned by [`Storage`] operations.
#[derive(Debug, Clone)]
pub struct StorageError {
    pub message: String,
    pub source: Option<String>,
}

impl StorageError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), source: None }
    }
    pub fn with_source(msg: impl Into<String>, src: impl Into<String>) -> Self {
        Self { message: msg.into(), source: Some(src.into()) }
    }
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(s) = &self.source { write!(f, ": {s}")?; }
        Ok(())
    }
}
impl std::error::Error for StorageError {}

// ---------------------------------------------------------------------------
// StorageKey
// ---------------------------------------------------------------------------

/// An opaque UUID-keyed storage address.
///
/// External [`Storage`] implementors receive this as a key argument.
/// Use [`StorageKey::as_uuid`] to derive a file-name or database key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StorageKey(pub(crate) Uuid);

impl StorageKey {
    /// Return the underlying UUID.
    pub fn as_uuid(&self) -> Uuid { self.0 }
    /// Construct a key from a UUID (for storage implementors and tests).
    pub fn from_uuid(id: Uuid) -> Self { Self(id) }
}

// ---------------------------------------------------------------------------
// StorageValue
// ---------------------------------------------------------------------------

/// Raw bytes stored in the backend.
///
/// External [`Storage`] implementors construct this from bytes on `get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageValue(pub(crate) Vec<u8>);

impl StorageValue {
    /// Construct from raw bytes.
    pub fn new(bytes: Vec<u8>) -> Self { Self(bytes) }
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}

// ---------------------------------------------------------------------------
// Storage trait
// ---------------------------------------------------------------------------

/// Async key-value backend.
///
/// Implement this trait to supply a custom persistence layer.
/// `StorageKey` and `StorageValue` are part of the public API so implementors
/// can inspect and construct them; the engine never exposes raw bytes itself.
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError>;
    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError>;
    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError>;
    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError>;

    /// Begin a checkpoint — subsequent writes may be staged.
    async fn checkpoint(&self) -> Result<(), StorageError> { Ok(()) }
    /// Make all staged writes durable.
    async fn commit(&self) -> Result<(), StorageError> { Ok(()) }
    /// Discard all staged writes since the last [`Storage::checkpoint`].
    async fn discard(&self) -> Result<(), StorageError> { Ok(()) }
}

// ---------------------------------------------------------------------------
// MemoryStorage
// ---------------------------------------------------------------------------

/// In-memory [`Storage`] implementation for tests and ephemeral use.
pub struct MemoryStorage {
    map: parking_lot::RwLock<std::collections::HashMap<Uuid, Vec<u8>>>,
    snapshot: parking_lot::Mutex<Option<std::collections::HashMap<Uuid, Vec<u8>>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            map: Default::default(),
            snapshot: parking_lot::Mutex::new(None),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError> {
        Ok(self.map.read().get(&key.0).map(|v| StorageValue(v.clone())))
    }
    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError> {
        self.map.write().insert(key.0, value.0);
        Ok(())
    }
    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        self.map.write().remove(&key.0);
        Ok(())
    }
    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError> {
        Ok(self.map.read().contains_key(&key.0))
    }
    async fn checkpoint(&self) -> Result<(), StorageError> {
        let mut snap = self.snapshot.lock();
        if snap.is_some() {
            return Err(StorageError::new("checkpoint already active"));
        }
        *snap = Some(self.map.read().clone());
        Ok(())
    }
    async fn commit(&self) -> Result<(), StorageError> {
        let mut snap = self.snapshot.lock();
        if snap.is_none() {
            return Err(StorageError::new("no active checkpoint"));
        }
        *snap = None;
        Ok(())
    }
    async fn discard(&self) -> Result<(), StorageError> {
        let mut snap = self.snapshot.lock();
        match snap.take() {
            Some(old) => { *self.map.write() = old; Ok(()) }
            None => Err(StorageError::new("no active checkpoint")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_set() {
        let store = MemoryStorage::new();
        let key = StorageKey::from_uuid(Uuid::new_v4());
        let val = StorageValue::new(vec![1, 2, 3]);
        store.set(&key, val.clone()).await.unwrap();
        let got = store.get(&key).await.unwrap().unwrap();
        assert_eq!(got, val);
    }

    #[tokio::test]
    async fn test_get_missing() {
        let store = MemoryStorage::new();
        let key = StorageKey::from_uuid(Uuid::new_v4());
        assert!(store.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete() {
        let store = MemoryStorage::new();
        let key = StorageKey::from_uuid(Uuid::new_v4());
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        assert!(store.contains(&key).await.unwrap());
        store.delete(&key).await.unwrap();
        assert!(!store.contains(&key).await.unwrap());
    }

    #[tokio::test]
    async fn test_contains() {
        let store = MemoryStorage::new();
        let key = StorageKey::from_uuid(Uuid::new_v4());
        assert!(!store.contains(&key).await.unwrap());
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        assert!(store.contains(&key).await.unwrap());
    }

    #[tokio::test]
    async fn test_overwrite() {
        let store = MemoryStorage::new();
        let key = StorageKey::from_uuid(Uuid::new_v4());
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.set(&key, StorageValue::new(vec![2, 3])).await.unwrap();
        let got = store.get(&key).await.unwrap().unwrap();
        assert_eq!(got.as_bytes(), &[2, 3]);
    }

    #[tokio::test]
    async fn test_checkpoint_commit_discard() {
        let store = MemoryStorage::new();
        let key = StorageKey::from_uuid(Uuid::new_v4());
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.checkpoint().await.unwrap();
        store.set(&key, StorageValue::new(vec![2])).await.unwrap();
        store.discard().await.unwrap();
        let got = store.get(&key).await.unwrap().unwrap();
        assert_eq!(got.as_bytes(), &[1]);
    }

    #[tokio::test]
    async fn test_checkpoint_commit_persists() {
        let store = MemoryStorage::new();
        let key = StorageKey::from_uuid(Uuid::new_v4());
        store.set(&key, StorageValue::new(vec![1])).await.unwrap();
        store.checkpoint().await.unwrap();
        store.set(&key, StorageValue::new(vec![2])).await.unwrap();
        store.commit().await.unwrap();
        let got = store.get(&key).await.unwrap().unwrap();
        assert_eq!(got.as_bytes(), &[2]);
    }

    #[tokio::test]
    async fn test_double_checkpoint_error() {
        let store = MemoryStorage::new();
        store.checkpoint().await.unwrap();
        assert!(store.checkpoint().await.unwrap_err().message.contains("already"));
    }

    #[tokio::test]
    async fn test_commit_without_checkpoint_error() {
        let store = MemoryStorage::new();
        assert!(store.commit().await.unwrap_err().message.contains("no active"));
    }

    #[tokio::test]
    async fn test_discard_without_checkpoint_error() {
        let store = MemoryStorage::new();
        assert!(store.discard().await.unwrap_err().message.contains("no active"));
    }
}
