//! Two-level value cache: in-memory + `Storage` backend.  All `pub(crate)`.
//!
//! ## Design: Loader as the Single I/O Point
//!
//! `Loader` is the only component that talks to [`Storage`].  It holds an
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
//!
//! ## Design: Persist-on-Compute
//!
//! Rather than a separate "save" phase, the scheduler calls `persist` for
//! each output value immediately after a transform wave completes.  Writes
//! go to the storage backend and respect any active checkpoint.

use std::sync::Arc;
use dashmap::DashMap;
use crate::node_id::NodeId;
use crate::storage::{Storage, StorageKey, StorageValue, StorageError, encode, decode,
                     PersistedNodeValue, PersistedElement};
use crate::value::{Value, ValueHash, ValueTypeRegistry};

pub(crate) struct Loader {
    storage:  Arc<dyn Storage>,
    cache:    DashMap<NodeId, (Value, ValueHash)>,
    registry: Arc<ValueTypeRegistry>,
}

impl Loader {
    /// Create a loader backed by the given storage and type registry.
    pub(crate) fn new(storage: Arc<dyn Storage>, registry: Arc<ValueTypeRegistry>) -> Self {
        Self { storage, cache: DashMap::new(), registry }
    }

    /// Get the value+hash for a node.  Memory-first, then storage.
    pub(crate) async fn get(&self, id: NodeId) -> Result<Option<(Value, ValueHash)>, StorageError> {
        if let Some(entry) = self.cache.get(&id) {
            return Ok(Some(entry.clone()));
        }
        let key = StorageKey::for_node(id);
        if let Some(sv) = self.storage.get(&key).await? {
            let rec: PersistedNodeValue = decode(sv.as_bytes())?;
            if let Ok(v) = self.registry.deserialize(&rec.type_key, &rec.value_bytes) {
                self.cache.insert(id, (v.clone(), rec.hash));
                return Ok(Some((v, rec.hash)));
            }
        }
        Ok(None)
    }

    /// Cache a value in memory (no storage write).
    pub(crate) fn cache(&self, id: NodeId, value: Value, hash: ValueHash) {
        self.cache.insert(id, (value, hash));
    }

    /// Persist a value to storage (also updates cache).
    pub(crate) async fn persist(&self, id: NodeId, value: &Value, hash: ValueHash)
        -> Result<(), StorageError>
    {
        let type_key    = self.registry.type_key_of(value);
        let value_bytes = self.registry.serialize(value);
        let rec = PersistedNodeValue { type_key, value_bytes, hash };
        let bytes = encode(&rec)?;
        self.storage.set(&StorageKey::for_node(id), StorageValue(bytes)).await?;
        self.cache.insert(id, (value.clone(), hash));
        Ok(())
    }

    /// Persist a collection element.
    pub(crate) async fn persist_element(
        &self,
        node: NodeId,
        elem_key: u64,
        value:    &Value,
        hash:     ValueHash,
    ) -> Result<(), StorageError> {
        let type_key    = self.registry.type_key_of(value);
        let value_bytes = self.registry.serialize(value);
        let rec = PersistedElement { key: elem_key, type_key, value_bytes, hash };
        let bytes = encode(&rec)?;
        self.storage.set(&StorageKey::for_element(node, elem_key), StorageValue(bytes)).await
    }

    /// Evict everything (called on `discard()`).
    pub(crate) fn evict_all(&self) { self.cache.clear(); }

    /// Flush all cached values to storage.
    pub(crate) async fn flush_all(&self) -> Result<(), StorageError> {
        let entries: Vec<(NodeId, Value, ValueHash)> = self.cache
            .iter().map(|e| (*e.key(), e.value().0.clone(), e.value().1)).collect();
        for (id, value, hash) in entries {
            self.persist(id, &value, hash).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use crate::value::{ValueTypeRegistry, ValueTypeRegistryBuilder};

    fn make_registry() -> Arc<ValueTypeRegistry> {
        let mut b = ValueTypeRegistryBuilder::new();
        b.register::<i32>().unwrap();
        b.register::<u32>().unwrap();
        Arc::new(b.freeze())
    }

    fn make_loader() -> Loader {
        let s = Arc::new(MemoryStorage::new());
        Loader::new(s, make_registry())
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
        let loader = Loader::new(Arc::clone(&s), Arc::clone(&registry));
        let id = NodeId::new();

        let v = registry.make_value(42i32).unwrap();
        let hash = crate::value::hash_value(&v, &registry);
        loader.persist(id, &v, hash).await.unwrap();

        // Evict from cache to force a storage round-trip.
        loader.evict_all();
        let loaded = loader.get(id).await.unwrap().expect("should be present");
        let n: i32 = registry.downcast_value::<i32>(&loaded.0).unwrap();
        assert_eq!(n, 42);
    }

    #[tokio::test]
    async fn evict_all_clears_cache() {
        let loader = make_loader();
        let id = NodeId::new();
        let v = loader.registry.make_value(1i32).unwrap();
        let h = crate::value::hash_value(&v, &loader.registry);
        loader.cache(id, v, h);
        assert!(loader.cache.contains_key(&id));
        loader.evict_all();
        assert!(!loader.cache.contains_key(&id));
    }
}