//! Type-erased value container and content-hash helpers.
//!
//! All types here are `pub(crate)`. External crates never touch `Value` directly.

use serde::{Serialize, de::DeserializeOwned};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

pub(crate) type ValueBox = Arc<dyn Any + Send + Sync + 'static>;

/// Type-erased, cheaply-cloneable node value.  Internal only.
#[derive(Clone, Debug)]
pub(crate) struct Value {
    data:     ValueBox,
    type_idx: usize,
}

impl Value {
    fn new(inner: ValueBox, type_idx: usize) -> Self { Self { data: inner, type_idx } }
    pub(crate) fn downcast_ref<T: Any>(&self) -> Option<&T> { self.data.downcast_ref::<T>() }
    pub(crate) fn type_idx(&self) -> usize { self.type_idx }
}

// ---------------------------------------------------------------------------
// ValueHash
// ---------------------------------------------------------------------------

pub(crate) type ValueHash = u64;

pub(crate) fn hash_bytes(bytes: &[u8]) -> ValueHash {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Hash a value by serialising it and hashing the bytes.
pub(crate) fn hash_value(v: &Value, reg: &ValueTypeRegistry) -> ValueHash {
    hash_bytes(&reg.serialize(v))
}

// ---------------------------------------------------------------------------
// RegistryError (pub(crate))
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct RegistryError {
    pub(crate) message: String,
}
impl RegistryError {
    pub(crate) fn new(msg: impl Into<String>) -> Self { Self { message: msg.into() } }
}
impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for RegistryError {}

// ---------------------------------------------------------------------------
// TypeEntry  (private)
// ---------------------------------------------------------------------------

struct TypeEntry {
    type_id:     TypeId,
    type_name:   &'static str,
    type_key:    String,
    serialize:   fn(&ValueBox) -> Vec<u8>,
    deserialize: fn(&[u8]) -> Result<ValueBox, String>,
}

impl TypeEntry {
    fn new<T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static>(
        type_key: String,
    ) -> Self {
        fn ser<T: Serialize + Any + Send + Sync + 'static>(vb: &ValueBox) -> Vec<u8> {
            rmp_serde::to_vec(vb.downcast_ref::<T>().expect("TypeEntry::ser type mismatch"))
                .unwrap_or_default()
        }
        fn de<T: DeserializeOwned + Any + Send + Sync + 'static>(
            bytes: &[u8],
        ) -> Result<ValueBox, String> {
            rmp_serde::from_slice::<T>(bytes)
                .map(|v| Arc::new(v) as ValueBox)
                .map_err(|e| e.to_string())
        }
        Self {
            type_id:     TypeId::of::<T>(),
            type_name:   std::any::type_name::<T>(),
            type_key,
            serialize:   ser::<T>,
            deserialize: de::<T>,
        }
    }

    fn make(&self, inner: ValueBox, idx: usize) -> Value { Value::new(inner, idx) }

    fn serialize_value(&self, v: &Value) -> Vec<u8> { (self.serialize)(&v.data) }

    fn deserialize_value(&self, bytes: &[u8], idx: usize) -> Result<Value, RegistryError> {
        (self.deserialize)(bytes)
            .map(|inner| Value::new(inner, idx))
            .map_err(|e| RegistryError::new(format!("deserialize `{}`: {e}", self.type_key)))
    }

    fn downcast<T: Any + Clone + 'static>(&self, v: &Value) -> Result<T, RegistryError> {
        v.downcast_ref::<T>()
            .cloned()
            .ok_or_else(|| RegistryError::new(format!(
                "type mismatch: expected `{}` ({})",
                self.type_name, self.type_key
            )))
    }
}

// ---------------------------------------------------------------------------
// RegistryStore  (private)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RegistryStore {
    entries:  Vec<TypeEntry>,
    by_key:   HashMap<String, usize>,
    by_type:  HashMap<TypeId, usize>,
}

impl RegistryStore {
    fn by_key(&self, k: &str)       -> Option<(usize, &TypeEntry)> {
        self.by_key.get(k).map(|&i| (i, &self.entries[i]))
    }
    fn by_type_id(&self, t: TypeId) -> Option<(usize, &TypeEntry)> {
        self.by_type.get(&t).map(|&i| (i, &self.entries[i]))
    }

    fn insert(&mut self, e: TypeEntry) -> Result<(), RegistryError> {
        if let Some((_, ex)) = self.by_type_id(e.type_id) {
            if ex.type_key != e.type_key {
                return Err(RegistryError::new(format!(
                    "type `{}` already registered under key {:?}",
                    e.type_name, ex.type_key
                )));
            }
            return Ok(()); // idempotent
        }
        if let Some((_, ex)) = self.by_key(&e.type_key) {
            if ex.type_id != e.type_id {
                return Err(RegistryError::new(format!(
                    "key {:?} already used by type `{}`",
                    e.type_key, ex.type_name
                )));
            }
            return Ok(());
        }
        let idx = self.entries.len();
        self.by_key.insert(e.type_key.clone(), idx);
        self.by_type.insert(e.type_id, idx);
        self.entries.push(e);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ValueTypeRegistryBuilder  (pub(crate), build phase only)
// ---------------------------------------------------------------------------

/// Mutable builder used exclusively during `EngineBuilder::build()`.
/// Single-threaded, no locking.  Call `freeze()` to produce a read-only
/// [`ValueTypeRegistry`] that can be shared across threads.
pub(crate) struct ValueTypeRegistryBuilder {
    store: RegistryStore,
}

impl ValueTypeRegistryBuilder {
    pub(crate) fn new() -> Self { Self { store: RegistryStore::default() } }

    /// Register `T`.  Idempotent for the same `(TypeId, type_name)` pair.
    pub(crate) fn register<T>(&mut self) -> Result<(), RegistryError>
    where T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let key = std::any::type_name::<T>().to_owned();
        self.store.insert(TypeEntry::new::<T>(key))
    }

    /// Seal the registry.  Consumes the builder; no further registration is possible.
    pub(crate) fn freeze(self) -> ValueTypeRegistry {
        ValueTypeRegistry {
            entries:  self.store.entries.into_boxed_slice(),
            by_key:   self.store.by_key,
            by_type:  self.store.by_type,
        }
    }
}

// ---------------------------------------------------------------------------
// ValueTypeRegistry  (pub(crate), runtime — frozen, lock-free)
// ---------------------------------------------------------------------------

/// Immutable type registry produced by [`ValueTypeRegistryBuilder::freeze()`].
/// Shared as `Arc<ValueTypeRegistry>` across the scheduler, loader, and
/// transform contexts.  All access is `&self`; no locking required.
pub(crate) struct ValueTypeRegistry {
    entries:  Box<[TypeEntry]>,
    by_key:   HashMap<String, usize>,
    by_type:  HashMap<TypeId, usize>,
}

impl ValueTypeRegistry {
    fn by_key(&self, k: &str)       -> Option<(usize, &TypeEntry)> {
        self.by_key.get(k).map(|&i| (i, &self.entries[i]))
    }
    fn by_type_id(&self, t: TypeId) -> Option<(usize, &TypeEntry)> {
        self.by_type.get(&t).map(|&i| (i, &self.entries[i]))
    }
    fn by_idx(&self, i: usize)      -> &TypeEntry { &self.entries[i] }

    pub(crate) fn make_value<T>(&self, v: T) -> Result<Value, RegistryError>
    where T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let (idx, entry) = self.by_type_id(TypeId::of::<T>())
            .ok_or_else(|| RegistryError::new(format!(
                "type `{}` not registered",
                std::any::type_name::<T>()
            )))?;
        Ok(entry.make(Arc::new(v), idx))
    }

    pub(crate) fn downcast_value<T>(&self, v: &Value) -> Result<T, RegistryError>
    where T: Any + Clone + 'static,
    {
        let (idx, entry) = self.by_type_id(TypeId::of::<T>())
            .ok_or_else(|| RegistryError::new(format!(
                "type `{}` not registered",
                std::any::type_name::<T>()
            )))?;
        if v.type_idx() != idx {
            return Err(RegistryError::new(format!(
                "type mismatch: expected `{}` (idx {}), got idx {}",
                entry.type_name, idx, v.type_idx()
            )));
        }
        entry.downcast::<T>(v)
    }

    pub(crate) fn serialize(&self, v: &Value) -> Vec<u8> {
        self.by_idx(v.type_idx()).serialize_value(v)
    }

    pub(crate) fn deserialize(&self, type_key: &str, bytes: &[u8]) -> Result<Value, RegistryError> {
        let (idx, entry) = self.by_key(type_key)
            .ok_or_else(|| RegistryError::new(format!("unknown type key {type_key:?}")))?;
        entry.deserialize_value(bytes, idx)
    }

    pub(crate) fn type_key_of(&self, v: &Value) -> String {
        self.by_idx(v.type_idx()).type_key.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> ValueTypeRegistry {
        let mut b = ValueTypeRegistryBuilder::new();
        b.register::<u32>().unwrap();
        b.register::<String>().unwrap();
        b.freeze()
    }

    #[test]
    fn make_and_downcast() {
        let r = reg();
        let v = r.make_value(42u32).unwrap();
        assert_eq!(r.downcast_value::<u32>(&v).unwrap(), 42u32);
    }

    #[test]
    fn type_mismatch_error() {
        let r = reg();
        let v = r.make_value(1u32).unwrap();
        assert!(r.downcast_value::<String>(&v).is_err());
    }

    #[test]
    fn serialize_round_trip() {
        let r = reg();
        let v = r.make_value(String::from("hello")).unwrap();
        let key = r.type_key_of(&v);
        let bytes = r.serialize(&v);
        let v2 = r.deserialize(&key, &bytes).unwrap();
        assert_eq!(r.downcast_value::<String>(&v2).unwrap(), "hello");
    }

    #[test]
    fn hash_is_content_based() {
        let r = reg();
        let v1 = r.make_value(99u32).unwrap();
        let v2 = r.make_value(99u32).unwrap();
        let v3 = r.make_value(100u32).unwrap();
        assert_eq!(hash_value(&v1, &r), hash_value(&v2, &r));
        assert_ne!(hash_value(&v1, &r), hash_value(&v3, &r));
    }

    #[test]
    fn register_is_idempotent() {
        let mut b = ValueTypeRegistryBuilder::new();
        b.register::<i32>().unwrap();
        b.register::<i32>().unwrap(); // no error
    }
}
