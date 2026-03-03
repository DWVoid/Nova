//! Async key-value storage trait + `MemoryStorage` (public) + internal helpers.
//!
//! Only `Storage`, `StorageError`, and `MemoryStorage` are public.
//! `StorageKey`, `StorageValue`, and serialisation helpers are `pub(crate)`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// StorageError (public)
// ---------------------------------------------------------------------------

/// Error returned by storage operations.
#[derive(Debug, Clone)]
pub struct StorageError {
    pub message: String,
    pub source:  Option<String>,
}

impl StorageError {
    pub fn new(msg: impl Into<String>) -> Self { Self { message: msg.into(), source: None } }
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
// Storage trait (public)
// ---------------------------------------------------------------------------

/// Async key-value backend.  Implement this to supply a custom storage engine.
///
/// `StorageKey` and `StorageValue` are `pub(crate)` — callers supply
/// `Arc<dyn Storage>` to the engine but never construct keys/values directly.
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError>;
    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError>;
    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError>;
    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError>;

    /// Begin a checkpoint.  Subsequent writes are staged.
    async fn checkpoint(&self) -> Result<(), StorageError> { Ok(()) }
    /// Make all staged writes durable.
    async fn commit(&self) -> Result<(), StorageError> { Ok(()) }
    /// Discard all staged writes since `checkpoint()`.
    async fn discard(&self) -> Result<(), StorageError> { Ok(()) }
}

// ---------------------------------------------------------------------------
// MemoryStorage (public)
// ---------------------------------------------------------------------------

/// In-memory `Storage` for tests and ephemeral use.
pub struct MemoryStorage {
    map:      parking_lot::RwLock<std::collections::HashMap<Uuid, Vec<u8>>>,
    snapshot: parking_lot::Mutex<Option<std::collections::HashMap<Uuid, Vec<u8>>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            map:      Default::default(),
            snapshot: parking_lot::Mutex::new(None),
        }
    }
}

impl Default for MemoryStorage { fn default() -> Self { Self::new() } }

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
        if snap.is_some() { return Err(StorageError::new("checkpoint already active")); }
        *snap = Some(self.map.read().clone());
        Ok(())
    }
    async fn commit(&self) -> Result<(), StorageError> {
        let mut snap = self.snapshot.lock();
        if snap.is_none() { return Err(StorageError::new("no active checkpoint")); }
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

// ---------------------------------------------------------------------------
// StorageKey / StorageValue — public for Storage implementors
// ---------------------------------------------------------------------------

/// A UUID-keyed storage address.
///
/// External [`Storage`] implementors receive this as a key argument.
/// Use [`StorageKey::as_uuid`] to derive a file-name or DB key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey(pub(crate) Uuid);

impl StorageKey {
    /// Return the underlying UUID.
    pub fn as_uuid(&self) -> Uuid { self.0 }
    /// Construct a key from any UUID (for storage implementors and tests).
    pub fn from_uuid(id: Uuid) -> Self { Self(id) }
    /// Key for a node's persisted value.
    pub(crate) fn for_node(id: crate::node_id::NodeId) -> Self { Self(id.as_uuid()) }
    /// Key for a collection element: XOR node UUID with element key.
    pub(crate) fn for_element(node: crate::node_id::NodeId, elem_key: u64) -> Self {
        let mut bytes = node.as_uuid().into_bytes();
        let ek = elem_key.to_le_bytes();
        for i in 0..8 { bytes[8 + i] ^= ek[i]; }
        Self(Uuid::from_bytes(bytes))
    }
    /// Key for the serialised WorkState snapshot.
    pub(crate) fn state() -> Self {
        // Fixed UUID: "nova-incremental workstate snapshot" (v5 of DNS namespace)
        const STATE_UUID: Uuid = Uuid::from_bytes([
            0x9a, 0x3f, 0x1c, 0x2e, 0x4b, 0x5d, 0x6e, 0x7f,
            0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ]);
        Self(STATE_UUID)
    }
}

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
// Serde helpers (pub(crate))
// ---------------------------------------------------------------------------

pub(crate) fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>, StorageError> {
    rmp_serde::to_vec(v)
        .map_err(|e| StorageError::with_source("encode", e.to_string()))
}

pub(crate) fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, StorageError> {
    rmp_serde::from_slice(bytes)
        .map_err(|e| StorageError::with_source("decode", e.to_string()))
}

/// Persisted value record for one node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedNodeValue {
    pub type_key:    String,
    pub value_bytes: Vec<u8>,
    pub hash:        crate::value::ValueHash,
}

/// Persisted value record for one collection element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedElement {
    pub key:         u64,
    pub type_key:    String,
    pub value_bytes: Vec<u8>,
    pub hash:        crate::value::ValueHash,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn set_get_delete() {
        let s = MemoryStorage::new();
        let key = StorageKey(Uuid::new_v4());
        let val = StorageValue(vec![1, 2, 3]);
        s.set(&key, val.clone()).await.unwrap();
        assert_eq!(s.get(&key).await.unwrap(), Some(val));
        s.delete(&key).await.unwrap();
        assert!(s.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn checkpoint_commit() {
        let s = MemoryStorage::new();
        let key = StorageKey(Uuid::new_v4());
        s.checkpoint().await.unwrap();
        s.set(&key, StorageValue(vec![1])).await.unwrap();
        s.commit().await.unwrap();
        assert_eq!(s.get(&key).await.unwrap(), Some(StorageValue(vec![1])));
    }

    #[tokio::test]
    async fn checkpoint_discard() {
        let s = MemoryStorage::new();
        let key = StorageKey(Uuid::new_v4());
        s.set(&key, StorageValue(vec![1])).await.unwrap();
        s.checkpoint().await.unwrap();
        s.set(&key, StorageValue(vec![2])).await.unwrap();
        s.discard().await.unwrap();
        assert_eq!(s.get(&key).await.unwrap(), Some(StorageValue(vec![1])));
    }

    #[tokio::test]
    async fn nested_checkpoint_rejected() {
        let s = MemoryStorage::new();
        s.checkpoint().await.unwrap();
        assert!(s.checkpoint().await.is_err());
        s.discard().await.unwrap();
    }
}