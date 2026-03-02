//! Lazy value loading and persistence helpers.
//!
//! ## Design: LazyLoader as the Single I/O Point
//!
//! `LazyLoader` is the only component that talks to [`Storage`].  It holds an
//! `Arc<ValueTypeRegistry>` so it can reconstruct fully-typed [`Value`]s from
//! stored bytes on cold load – no `Vec<u8>` wrapping ever leaks out.
//!
//! ## Design: Two-Level Cache
//!
//! 1. **In-memory shard cache** (`DashMap<NodeId, Value>`) – hot typed values.
//! 2. **Persistent storage** (user-supplied [`Storage`] impl) – values that
//!    survive process restarts.
//!
//! On `get`, we check the in-memory cache first; if absent, we decode from
//! storage using the registry to obtain a typed `Value`.  On `persist`, we
//! write both `type_key` and serialised bytes to storage.

use std::sync::Arc;
use dashmap::DashMap;
use crate::node_id::NodeId;
use crate::value::{Value, ValueHash, ValueTypeRegistry, hash_bytes};
use crate::storage::{Storage, StorageKey, StorageValue, StorageError, PersistedNodeData, encode, decode};

/// Lazy value loader backed by an async [`Storage`] and an in-memory cache.
#[derive(Clone)]
pub struct LazyLoader {
    storage: Arc<dyn Storage>,
    cache: Arc<DashMap<NodeId, Value>>,
    registry: Arc<ValueTypeRegistry>,
}

impl LazyLoader {
    /// Create a loader backed by the given storage and type registry.
    pub fn new(storage: Arc<dyn Storage>, registry: Arc<ValueTypeRegistry>) -> Self {
        Self {
            storage,
            cache: Arc::new(DashMap::new()),
            registry,
        }
    }

    /// Return `true` if `id` is present in the in-memory cache.
    pub fn is_cached(&self, id: NodeId) -> bool {
        self.cache.contains_key(&id)
    }

    /// Return a reference to the value type registry.
    pub fn registry(&self) -> &ValueTypeRegistry {
        &self.registry
    }

    /// Retrieve the value for `id` from the in-memory cache, falling back to
    /// storage.
    ///
    /// Returns `Ok(None)` if the node has never been persisted or computed.
    ///
    /// Values loaded from cold storage are **fully typed** – the registry
    /// deserialises them to their original concrete type using the stored
    /// `type_key`.
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
                // Reconstruct the fully-typed Value via the registry.
                let v = self.registry
                    .deserialize_value(&data.type_key, &data.value_bytes)
                    .map_err(|e| StorageError::with_source(
                        format!("failed to deserialise node {id} (type_key={:?})", data.type_key),
                        e.message,
                    ))?;
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

    /// Persist a [`Value`] to storage, storing its `type_key` alongside the
    /// serialised bytes so reload can reconstruct the fully-typed value.
    pub async fn persist(
        &self,
        id: NodeId,
        value: Value,
        hash: ValueHash,
        is_input: bool,
    ) -> Result<(), StorageError> {
        let type_key = self.registry.type_key_of(&value).to_string();
        let value_bytes = self.registry.serialize_value(&value);

        // Update in-memory cache.
        self.cache.insert(id, value);

        let data = PersistedNodeData {
            node_id: id,
            type_key,
            value_bytes,
            value_hash: hash,
            is_input,
        };
        let bytes = encode(&data)?;
        self.storage.set(&StorageKey::for_node(id), StorageValue::new(bytes)).await
    }

    /// Drop the in-memory cache entry for `id` to free memory.
    pub fn evict(&self, id: NodeId) {
        self.cache.remove(&id);
    }

    /// Place a value into the cache without persisting to storage.
    pub fn cache_value(&self, id: NodeId, value: Value) {
        self.cache.insert(id, value);
    }

    /// Return a reference to the underlying storage.
    pub fn storage(&self) -> &Arc<dyn Storage> {
        &self.storage
    }

    /// Flush all in-memory cached values to storage.
    pub async fn flush_all(&self) -> Result<(), StorageError> {
        let entries: Vec<(NodeId, Value)> = self.cache.iter()
            .map(|e| (*e.key(), e.value().clone()))
            .collect();
        for (id, value) in entries {
            let type_key = self.registry.type_key_of(&value).to_string();
            let value_bytes = self.registry.serialize_value(&value);
            let hash = hash_bytes(&value_bytes);
            let data = PersistedNodeData {
                node_id: id,
                type_key,
                value_bytes,
                value_hash: hash,
                is_input: false,
            };
            let bytes = encode(&data)?;
            self.storage.set(&StorageKey::for_node(id), StorageValue::new(bytes)).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use crate::value::{ValueTypeRegistry, hash_bytes};
    use std::sync::Arc;

    fn make_registry() -> Arc<ValueTypeRegistry> {
        let mut r = ValueTypeRegistry::new();
        r.register_primitives().unwrap();
        Arc::new(r)
    }

    fn make_loader() -> LazyLoader {
        let s = Arc::new(MemoryStorage::new());
        LazyLoader::new(s, make_registry())
    }

    #[tokio::test]
    async fn get_returns_none_when_absent() {
        let loader = make_loader();
        let id = NodeId::new();
        assert!(loader.get(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn persist_then_get_returns_typed_value() {
        let registry = make_registry();
        let s = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
        let loader = LazyLoader::new(Arc::clone(&s), Arc::clone(&registry));
        let id = NodeId::new();

        let v = registry.make_value(42i32).unwrap();
        let bytes = registry.serialize_value(&v);
        let hash = hash_bytes(&bytes);
        loader.persist(id, v, hash, false).await.unwrap();

        // Evict from cache to force a storage round-trip.
        loader.evict(id);
        let loaded = loader.get(id).await.unwrap().expect("should be present");

        // After cold reload, value must be typed i32, not Vec<u8>.
        assert_eq!(&*registry.type_key_of(&loaded), "i32");
        assert_eq!(registry.downcast_value::<i32>(&loaded).unwrap(), 42i32);
    }

    #[tokio::test]
    async fn persist_bytes_are_serde_not_raw_memory() {
        let registry = make_registry();
        let s = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
        let loader = LazyLoader::new(Arc::clone(&s), Arc::clone(&registry));
        let id = NodeId::new();

        let v = registry.make_value(99u64).unwrap();
        let bytes = registry.serialize_value(&v);
        let hash = hash_bytes(&bytes);
        loader.persist(id, v, hash, false).await.unwrap();

        let data = loader.load_node_data(id).await.unwrap().unwrap();
        assert_eq!(data.type_key, "u64");
        let decoded: u64 = rmp_serde::from_slice(&data.value_bytes).expect("must be valid msgpack");
        assert_eq!(decoded, 99u64);
    }

    #[tokio::test]
    async fn cache_hit_avoids_storage_call() {
        let registry = make_registry();
        let s = Arc::new(MemoryStorage::new());
        let loader = LazyLoader::new(Arc::clone(&s) as Arc<dyn Storage>, Arc::clone(&registry));
        let id = NodeId::new();
        let v = registry.make_value(42i32).unwrap();
        loader.cache_value(id, v);
        let result = loader.get(id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(registry.downcast_value::<i32>(&result.unwrap()).unwrap(), 42i32);
    }

    #[tokio::test]
    async fn evict_clears_cache() {
        let loader = make_loader();
        let registry = make_registry();
        let id = NodeId::new();
        loader.cache_value(id, registry.make_value(1u32).unwrap());
        loader.evict(id);
        assert!(loader.get(id).await.unwrap().is_none());
    }

    #[test]
    fn serialize_produces_consistent_hash() {
        let registry = make_registry();
        let v = registry.make_value(42u32).unwrap();
        let h1 = hash_bytes(&registry.serialize_value(&v));
        let h2 = hash_bytes(&registry.serialize_value(&v));
        assert_eq!(h1, h2);
    }
}
