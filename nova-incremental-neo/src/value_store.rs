//! [`ValueStore`] — in-memory cache of deserialized values, backed by [`Storage`].
//!
//! Values are stored as type-erased `Arc<dyn Any + Send + Sync>` paired with
//! serialization metadata.  On cache miss, bytes are loaded from storage and
//! deserialized on demand.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use dashmap::DashMap;
use serde::{Serialize, Deserialize};

use crate::keys::{SlotStateKey, ElementKey};
use crate::storage::{Storage, StorageError, StorageValue};
use crate::transform::{ErasedValue, IncrementalValue, hash_bytes, serialize_erased};
use crate::workstate::ValueHash;

// ---------------------------------------------------------------------------
// Cached entry
// ---------------------------------------------------------------------------

/// One cached slot value: the type-erased value, its hash, and raw bytes for
/// flushing to storage.
struct CachedValue {
    value: ErasedValue,
    hash: ValueHash,
    /// Serialized bytes. `None` until the value has been written (new or changed).
    dirty_bytes: Option<Vec<u8>>,
    /// Stable type name, used as a discriminant in the persisted record.
    type_name: &'static str,
}

// ---------------------------------------------------------------------------
// Persisted record format
// ---------------------------------------------------------------------------

/// On-disk format for a single slot value.
#[derive(Serialize, Deserialize)]
pub(crate) struct PersistedValue {
    pub(crate) type_name: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) hash: ValueHash,
}

/// On-disk format for a collection index (list of element keys).
#[derive(Serialize, Deserialize)]
pub(crate) struct PersistedIndex {
    pub(crate) element_keys: Vec<u64>,
}

// ---------------------------------------------------------------------------
// ValueStore
// ---------------------------------------------------------------------------

/// In-memory cache of current values, keyed by [`SlotStateKey`] or [`ElementKey`].
///
/// Does not own the `Storage` backend; callers pass `&dyn Storage` to operations
/// that may need to load from or flush to disk.
pub(crate) struct ValueStore {
    /// Cached single-value slots.
    single: DashMap<SlotStateKey, CachedValue>,
    /// Cached collection element values.
    elements: DashMap<ElementKey, CachedValue>,
    /// Dirty flags: keys that need flushing on commit.
    dirty_single: DashMap<SlotStateKey, ()>,
    dirty_elements: DashMap<ElementKey, ()>,
    /// Dirty collection indexes (need re-writing).
    dirty_indexes: DashMap<SlotStateKey, ()>,
}

impl ValueStore {
    pub(crate) fn new() -> Self {
        Self {
            single: DashMap::new(),
            elements: DashMap::new(),
            dirty_single: DashMap::new(),
            dirty_elements: DashMap::new(),
            dirty_indexes: DashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Single-value operations
    // -----------------------------------------------------------------------

    /// Store a concrete value in the cache.  Serializes eagerly for hash + flush.
    pub(crate) fn set<T: IncrementalValue>(
        &self,
        key: SlotStateKey,
        value: T,
    ) -> Result<ValueHash, String> {
        let bytes = serialize_erased(&value)?;
        let hash = hash_bytes(&bytes);
        self.single.insert(key, CachedValue {
            value: Arc::new(value) as ErasedValue,
            hash,
            dirty_bytes: Some(bytes),
            type_name: std::any::type_name::<T>(),
        });
        self.dirty_single.insert(key, ());
        Ok(hash)
    }

    /// Store a pre-erased value with pre-computed bytes and hash.
    pub(crate) fn set_erased(
        &self,
        key: SlotStateKey,
        value: ErasedValue,
        bytes: Vec<u8>,
        hash: ValueHash,
        type_name: &'static str,
    ) {
        self.single.insert(key, CachedValue {
            value,
            hash,
            dirty_bytes: Some(bytes),
            type_name,
        });
        self.dirty_single.insert(key, ());
    }

    /// Get a typed reference to a cached single value (cache hit only).
    pub(crate) fn get<T: IncrementalValue>(&self, key: SlotStateKey) -> Option<Arc<T>> {
        self.single.get(&key).and_then(|e| {
            Arc::clone(&e.value).downcast::<T>().ok()
        })
    }

    /// Get the erased value and hash for a slot (cache hit only).
    pub(crate) fn get_erased(&self, key: SlotStateKey) -> Option<(ErasedValue, ValueHash)> {
        self.single.get(&key).map(|e| (Arc::clone(&e.value), e.hash))
    }

    /// Load a single value from storage if not in cache. Returns the erased value + hash.
    pub(crate) async fn load_erased(
        &self,
        key: SlotStateKey,
        storage: &dyn Storage,
        deserialize: fn(&[u8]) -> Result<ErasedValue, String>,
        type_name: &'static str,
    ) -> Result<Option<(ErasedValue, ValueHash)>, StorageError> {
        if let Some(entry) = self.single.get(&key) {
            return Ok(Some((Arc::clone(&entry.value), entry.hash)));
        }
        let storage_key = key.to_storage_key();
        let Some(sv) = storage.get(&storage_key).await? else { return Ok(None); };
        let record: PersistedValue = rmp_serde::from_slice(sv.as_bytes())
            .map_err(|e| StorageError::with_source("deserialize slot", e.to_string()))?;
        let value = deserialize(&record.bytes)
            .map_err(|e| StorageError::with_source("downcast slot", e))?;
        let hash = record.hash;
        // Populate cache (no dirty flag — loaded from storage).
        self.single.insert(key, CachedValue {
            value: Arc::clone(&value),
            hash,
            dirty_bytes: None,
            type_name,
        });
        Ok(Some((value, hash)))
    }

    // -----------------------------------------------------------------------
    // Collection element operations
    // -----------------------------------------------------------------------

    /// Store one collection element in cache.
    pub(crate) fn set_element<T: IncrementalValue>(
        &self,
        ek: ElementKey,
        value: T,
    ) -> Result<ValueHash, String> {
        let bytes = serialize_erased(&value)?;
        let hash = hash_bytes(&bytes);
        self.elements.insert(ek, CachedValue {
            value: Arc::new(value) as ErasedValue,
            hash,
            dirty_bytes: Some(bytes),
            type_name: std::any::type_name::<T>(),
        });
        self.dirty_elements.insert(ek, ());
        self.dirty_indexes.insert(ek.slot, ());
        Ok(hash)
    }

    /// Store a pre-erased collection element.
    pub(crate) fn set_element_erased(
        &self,
        ek: ElementKey,
        value: ErasedValue,
        bytes: Vec<u8>,
        hash: ValueHash,
        type_name: &'static str,
    ) {
        self.elements.insert(ek, CachedValue {
            value,
            hash,
            dirty_bytes: Some(bytes),
            type_name,
        });
        self.dirty_elements.insert(ek, ());
        self.dirty_indexes.insert(ek.slot, ());
    }

    /// Remove a collection element from cache.
    pub(crate) fn remove_element(&self, ek: ElementKey) {
        self.elements.remove(&ek);
        self.dirty_indexes.insert(ek.slot, ());
    }

    /// Get the erased value + hash for a collection element (cache hit only).
    pub(crate) fn get_element_erased(&self, ek: ElementKey) -> Option<(ErasedValue, ValueHash)> {
        self.elements.get(&ek).map(|e| (Arc::clone(&e.value), e.hash))
    }

    /// Get all cached elements for a collection slot, sorted by element key.
    pub(crate) fn get_all_elements_erased(
        &self,
        slot: SlotStateKey,
        element_keys: &[u64],
    ) -> Vec<(u64, ErasedValue, ValueHash)> {
        element_keys.iter().filter_map(|&ek_u64| {
            let ek = ElementKey::new(slot, ek_u64);
            self.elements.get(&ek).map(|e| (ek_u64, Arc::clone(&e.value), e.hash))
        }).collect()
    }

    // -----------------------------------------------------------------------
    // Flush to storage (commit)
    // -----------------------------------------------------------------------

    /// Write all dirty cache entries to storage.
    pub(crate) async fn flush(&self, storage: &dyn Storage) -> Result<(), StorageError> {
        // Flush dirty single-value slots.
        let dirty_single_keys: Vec<SlotStateKey> =
            self.dirty_single.iter().map(|e| *e.key()).collect();
        for key in dirty_single_keys {
            if let Some(entry) = self.single.get(&key) {
                if let Some(bytes) = &entry.dirty_bytes {
                    let record = PersistedValue {
                        type_name: entry.type_name.to_owned(),
                        bytes: bytes.clone(),
                        hash: entry.hash,
                    };
                    let encoded = rmp_serde::to_vec(&record)
                        .map_err(|e| StorageError::with_source("encode slot", e.to_string()))?;
                    storage.set(&key.to_storage_key(), StorageValue::new(encoded)).await?;
                }
            }
            self.dirty_single.remove(&key);
        }

        // Flush dirty collection elements.
        let dirty_elem_keys: Vec<ElementKey> =
            self.dirty_elements.iter().map(|e| *e.key()).collect();
        for ek in dirty_elem_keys {
            if let Some(entry) = self.elements.get(&ek) {
                if let Some(bytes) = &entry.dirty_bytes {
                    let record = PersistedValue {
                        type_name: entry.type_name.to_owned(),
                        bytes: bytes.clone(),
                        hash: entry.hash,
                    };
                    let encoded = rmp_serde::to_vec(&record)
                        .map_err(|e| StorageError::with_source("encode element", e.to_string()))?;
                    storage.set(&ek.to_storage_key(), StorageValue::new(encoded)).await?;
                }
            }
            self.dirty_elements.remove(&ek);
        }

        // Flush dirty collection indexes.
        let dirty_idx_keys: Vec<SlotStateKey> =
            self.dirty_indexes.iter().map(|e| *e.key()).collect();
        for slot_key in dirty_idx_keys {
            // Collect all element keys cached for this slot.
            let mut elem_keys: Vec<u64> = self.elements.iter()
                .filter(|e| e.key().slot == slot_key)
                .map(|e| e.key().element_key)
                .collect();
            elem_keys.sort_unstable();
            let index = PersistedIndex { element_keys: elem_keys };
            let encoded = rmp_serde::to_vec(&index)
                .map_err(|e| StorageError::with_source("encode index", e.to_string()))?;
            storage.set(&slot_key.collection_index_storage_key(), StorageValue::new(encoded)).await?;
            self.dirty_indexes.remove(&slot_key);
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Evict (for discard)
    // -----------------------------------------------------------------------

    /// Remove all cached entries, forcing reload from storage on next access.
    pub(crate) fn evict_all(&self) {
        self.single.clear();
        self.elements.clear();
        self.dirty_single.clear();
        self.dirty_elements.clear();
        self.dirty_indexes.clear();
    }
}
