//! Lazy value loading and persistence helpers.
//!
//! ## Design: LazyLoader as the Single I/O Point
//!
//! `LazyLoader` is the only component that talks to [`Storage`].  The graph
//! stores node values in-memory (`NodeEntry::value`) but those slots are
//! populated on-demand by the loader.  This "lazy pull" pattern keeps memory
//! usage bounded: cold nodes (those not accessed since startup) never consume
//! RAM.
//!
//! ## Design: Two-Level Cache
//!
//! 1. **In-memory shard cache** (`DashMap<NodeId, Value>`) – hot values.
//! 2. **Persistent storage** (user-supplied [`Storage`] impl) – values that
//!    survive process restarts.
//!
//! On `get`, we check the in-memory cache first; if absent, we decode from
//! storage and populate the cache.  On `persist`, we write to storage and
//! update the cache.  On `evict`, we drop the in-memory entry (the value
//! remains in storage).
//!
//! ## Design: Serde-Safe Bytes
//!
//! All persistence goes through [`Value::to_bytes`], which uses the
//! `Serialize` implementation captured at [`Value::new`] time.  The raw
//! bytes stored in the key-value store are therefore always a valid
//! MessagePack encoding of the original type — never a raw memory dump.
//! This guarantees that bytes can be safely deserialized after a process
//! restart or on a different machine.

use std::sync::Arc;
use dashmap::DashMap;
use crate::incremental::node_id::NodeId;
use crate::incremental::value::{Value, ValueHash, hash_bytes};
use crate::incremental::storage::{Storage, StorageKey, StorageValue, StorageError, PersistedNodeData, encode, decode};

/// Lazy value loader backed by an async [`Storage`] and an in-memory cache.
#[derive(Clone)]
pub struct LazyLoader {
    storage: Arc<dyn Storage>,
    cache: Arc<DashMap<NodeId, Value>>,
}

impl LazyLoader {
    /// Create a loader backed by the given storage.
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            cache: Arc::new(DashMap::new()),
        }
    }

    /// Return `true` if `id` is present in the in-memory cache.
    ///
    /// This is a fast synchronous check used by the scheduler to determine
    /// whether a source node has a usable value without awaiting storage.
    pub fn is_cached(&self, id: NodeId) -> bool {
        self.cache.contains_key(&id)
    }

    /// Retrieve the value for `id` from the in-memory cache, falling back to
    /// storage.
    ///
    /// Returns `Ok(None)` if the node has never been persisted or computed.
    ///
    /// **Note**: values loaded from cold storage are returned as opaque
    /// `Vec<u8>` wrapped in a `Value`.  If the caller needs the original typed
    /// value, they should use [`Value::from_bytes::<T>`] on the result.
    pub async fn get(&self, id: NodeId) -> Result<Option<Value>, StorageError> {
        // 1. In-memory cache hit.
        if let Some(v) = self.cache.get(&id) {
            return Ok(Some(v.clone()));
        }
        // 2. Storage lookup.
        let key = StorageKey::for_node(id);
        match self.storage.get(&key).await? {
            None => Ok(None),
            Some(sv) => {
                let data: PersistedNodeData = decode(sv.as_bytes())?;
                if data.value_bytes.is_empty() {
                    return Ok(None);
                }
                // Wrap the raw bytes as a Value<Vec<u8>>.
                // The bytes are a valid serde encoding of the original type;
                // callers that need the typed value should use
                // Value::from_bytes::<T>(&bytes) after obtaining the bytes.
                let v = Value::new(data.value_bytes);
                self.cache.insert(id, v.clone());
                Ok(Some(v))
            }
        }
    }

    /// Load raw persisted data for a node (used during graph restore).
    pub async fn load_node_data(&self, id: NodeId) -> Result<Option<PersistedNodeData>, StorageError> {
        let key = StorageKey::for_node(id);
        match self.storage.get(&key).await? {
            None => Ok(None),
            Some(sv) => Ok(Some(decode(sv.as_bytes())?)),
        }
    }

    /// Persist a [`Value`] to storage using its built-in serde serialisation.
    ///
    /// Serialisation is performed by calling [`Value::to_bytes`], which uses
    /// the `Serialize` closure captured when the value was created with
    /// [`Value::new`].  No raw memory is written; the bytes are always a
    /// valid MessagePack encoding.
    ///
    /// The value is also inserted into the in-memory cache so subsequent
    /// `get` calls are fast.
    pub async fn persist(
        &self,
        id: NodeId,
        value: Value,
        hash: ValueHash,
        is_input: bool,
    ) -> Result<(), StorageError> {
        // Serialise via the serde closure captured at Value::new time.
        let value_bytes = value.to_bytes();

        // Update in-memory cache.
        self.cache.insert(id, value);

        // Write to storage.
        let data = PersistedNodeData {
            node_id: id,
            value_bytes,
            value_hash: hash,
            is_input,
        };
        let bytes = encode(&data)?;
        self.storage.set(&StorageKey::for_node(id), StorageValue::new(bytes)).await
    }

    /// Drop the in-memory cache entry for `id` to free memory.
    ///
    /// The value remains in storage and will be reloaded on next access.
    pub fn evict(&self, id: NodeId) {
        self.cache.remove(&id);
    }

    /// Place a value into the cache without persisting to storage.
    ///
    /// Useful for in-memory-only nodes or when the caller will batch-persist
    /// later.
    pub fn cache_value(&self, id: NodeId, value: Value) {
        self.cache.insert(id, value);
    }

    /// Return a reference to the underlying storage.
    pub fn storage(&self) -> &Arc<dyn Storage> {
        &self.storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::storage::MemoryStorage;
    use crate::incremental::value::hash_bytes;
    use std::sync::Arc;

    #[tokio::test]
    async fn get_returns_none_when_absent() {
        let s = Arc::new(MemoryStorage::new());
        let loader = LazyLoader::new(s);
        let id = NodeId::new();
        assert!(loader.get(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn persist_then_get_returns_serde_bytes() {
        let s = Arc::new(MemoryStorage::new());
        let loader = LazyLoader::new(s);
        let id = NodeId::new();

        let original = 42i32;
        let v = Value::new(original);
        let bytes = v.to_bytes();
        let hash = hash_bytes(&bytes);
        loader.persist(id, v, hash, false).await.unwrap();

        // Evict from cache to force a storage round-trip.
        loader.evict(id);
        let loaded = loader.get(id).await.unwrap().expect("should be present");

        // After cold reload, value is Vec<u8>; the bytes are valid serde data.
        let raw = loaded.downcast::<Vec<u8>>().expect("cold load wraps as Vec<u8>");
        let restored = Value::from_bytes::<i32>(raw).unwrap();
        assert_eq!(restored.downcast::<i32>(), Some(&42i32));
    }

    #[tokio::test]
    async fn persist_bytes_are_serde_not_raw_memory() {
        let s = Arc::new(MemoryStorage::new());
        let loader = LazyLoader::new(s);
        let id = NodeId::new();

        let v = Value::new(99u64);
        let bytes = v.to_bytes();
        let hash = hash_bytes(&bytes);
        loader.persist(id, v.clone(), hash, false).await.unwrap();

        // Verify the stored bytes are valid msgpack for u64, not a raw pointer.
        let data = loader.load_node_data(id).await.unwrap().unwrap();
        let decoded: u64 = rmp_serde::from_slice(&data.value_bytes).expect("must be valid msgpack");
        assert_eq!(decoded, 99u64);
    }

    #[tokio::test]
    async fn cache_hit_avoids_storage_call() {
        let s = Arc::new(MemoryStorage::new());
        let loader = LazyLoader::new(Arc::clone(&s) as Arc<dyn Storage>);
        let id = NodeId::new();
        let v = Value::new(42i32);
        loader.cache_value(id, v.clone());
        // Storage is empty, but cache has the value.
        let result = loader.get(id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().downcast::<i32>(), Some(&42i32));
    }

    #[tokio::test]
    async fn evict_clears_cache() {
        let s = Arc::new(MemoryStorage::new());
        let loader = LazyLoader::new(s);
        let id = NodeId::new();
        loader.cache_value(id, Value::new(1u32));
        loader.evict(id);
        // After evict, storage miss → None.
        assert!(loader.get(id).await.unwrap().is_none());
    }

    #[test]
    fn to_bytes_produces_consistent_hash() {
        let v = Value::new(42u32);
        let h1 = hash_bytes(&v.to_bytes());
        let h2 = hash_bytes(&v.to_bytes());
        assert_eq!(h1, h2);
    }
}