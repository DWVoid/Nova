//! Type-erased value container and content-hash helpers.
//!
//! `Value` is an internal implementation detail of the incremental engine.
//! Users of the engine never construct or inspect `Value` directly; all
//! interaction goes through the typed API on [`crate::engine::IncrementalEngine`].
//!
//! ## Design: Type Erasure with Registry-Backed Serde
//!
//! Every `Value` carries a `type_idx` — a stable integer index into the
//! engine-private [`ValueTypeRegistry`]'s entry vec.  All type-related
//! operations (serialize, deserialize, downcast, key lookup) are routed
//! through the registry, which resolves the index in O(1).
//!
//! ## Design: Content Hash
//!
//! [`hash_bytes`] applies [`DefaultHasher`] over the serialized bytes.
//! Hashing the canonical byte representation means two logically-equal values
//! produced by separate transform invocations will produce the same hash and
//! trigger the early-exit optimisation.

use serde::{Serialize, de::DeserializeOwned};
use std::any::{Any, TypeId};
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

type ValueBox = Arc<dyn Any + Send + Sync + 'static>;

/// A type-erased, cheaply-cloneable, serde-safe node value.
///
/// **This is an internal type.**  Users interact with the engine through the
/// typed APIs (`add_input`, `set_input`, `get_value`) and never hold `Value`
/// directly.
#[derive(Clone)]
pub(crate) struct Value {
    data: ValueBox,
    type_idx: usize,
}

impl Value {
    fn new(inner: ValueBox, type_idx: usize) -> Self {
        Self { data: inner, type_idx }
    }

    fn downcast<T: Any>(&self) -> Option<&T> {
        self.data.downcast_ref::<T>()
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Value(type_idx={})", self.type_idx)
    }
}

// ---------------------------------------------------------------------------
// ValueHash
// ---------------------------------------------------------------------------

/// A 64-bit content hash derived from a value's serialised representation.
pub type ValueHash = u64;

/// Compute a [`ValueHash`] by hashing the **serialised bytes** of a value.
pub(crate) fn hash_bytes(bytes: &[u8]) -> ValueHash {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

// ---------------------------------------------------------------------------
// ValueTypeRegistry
// ---------------------------------------------------------------------------

/// Error produced by [`ValueTypeRegistry`] operations.
#[derive(Debug, Clone)]
pub(crate) struct RegistryError {
    pub(crate) message: String,
}

impl RegistryError {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RegistryError {}

// ---------------------------------------------------------------------------
// TypeEntry
// ---------------------------------------------------------------------------

struct TypeEntry {
    type_id: TypeId,
    type_name: &'static str,
    type_key: String,
    serialize: fn(&ValueBox) -> Result<Vec<u8>, rmp_serde::encode::Error>,
    deserialize: fn(&[u8]) -> Result<ValueBox, rmp_serde::decode::Error>,
}

impl TypeEntry {
    fn new<T>(type_key: String) -> Self
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        fn serialize_fn<T: Serialize + Any + Send + Sync + 'static>(
            value: &ValueBox,
        ) -> Result<Vec<u8>, rmp_serde::encode::Error> {
            rmp_serde::to_vec(
                value
                    .downcast_ref::<T>()
                    .expect("TypeEntry::serialize_fn: type_idx mismatch"),
            )
        }

        fn deserialize_fn<T: DeserializeOwned + Any + Send + Sync + 'static>(
            bytes: &[u8],
        ) -> Result<ValueBox, rmp_serde::decode::Error> {
            Ok(Arc::new(rmp_serde::from_slice::<T>(bytes)?))
        }

        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            type_key,
            serialize: serialize_fn::<T>,
            deserialize: deserialize_fn::<T>,
        }
    }

    /// Wrap a concrete `T` in a `Value` using `idx` as the type identifier.
    fn make_value<T: Any + Send + Sync + 'static>(&self, v: T, idx: usize) -> Value {
        Value::new(Arc::new(v), idx)
    }

    /// Serialise `value` to bytes.
    fn serialize(&self, value: &Value) -> Vec<u8> {
        (self.serialize)(&value.data).unwrap_or_else(|e| {
            panic!(
                "TypeEntry::serialize: failed to serialise `{}` (key {:?}): {}",
                self.type_name, self.type_key, e
            )
        })
    }

    /// Deserialize `bytes` into a `Value` tagged with `idx`.
    fn deserialize(&self, bytes: &[u8], idx: usize) -> Result<Value, RegistryError> {
        (self.deserialize)(bytes)
            .map(|inner| Value::new(inner, idx))
            .map_err(|e| {
                RegistryError::new(format!(
                    "failed to deserialize `{}` (key {:?}): {}",
                    self.type_name, self.type_key, e
                ))
            })
    }

    /// Downcast `value` to an owned `T`.
    fn downcast_value<T: Any + Clone + 'static>(&self, value: &Value) -> Result<T, RegistryError> {
        value.downcast::<T>().cloned().ok_or_else(|| {
            RegistryError::new(format!(
                "type mismatch – expected `{}` (key {:?})",
                self.type_name, self.type_key
            ))
        })
    }
}

// ---------------------------------------------------------------------------
// RegistryStore
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RegistryStore {
    entries: Vec<TypeEntry>,
    by_key: std::collections::HashMap<String, usize>,
    by_type: std::collections::HashMap<TypeId, usize>,
}

impl RegistryStore {
    fn get_by_key(&self, key: &str) -> Option<(usize, &TypeEntry)> {
        self.by_key.get(key).map(|&i| (i, &self.entries[i]))
    }
    fn get_by_type(&self, tid: TypeId) -> Option<(usize, &TypeEntry)> {
        self.by_type.get(&tid).map(|&i| (i, &self.entries[i]))
    }
    fn get_by_idx(&self, idx: usize) -> &TypeEntry {
        &self.entries[idx]
    }

    fn insert(&mut self, entry: TypeEntry) -> Result<(), RegistryError> {
        if let Some((_, existing)) = self.get_by_type(entry.type_id) {
            if existing.type_key != entry.type_key {
                return Err(RegistryError::new(format!(
                    "type `{}` is already registered under key {:?}, \
                     cannot re-register under {:?}",
                    entry.type_name, existing.type_key, entry.type_key
                )));
            }
            return Ok(());
        }
        if let Some((_, existing)) = self.get_by_key(&entry.type_key) {
            if existing.type_id != entry.type_id {
                return Err(RegistryError::new(format!(
                    "key {:?} is already registered for type `{}`, \
                     cannot re-register for type `{}`",
                    entry.type_key, existing.type_name, entry.type_name
                )));
            }
            return Ok(());
        }
        let idx = self.entries.len();
        self.by_key.insert(entry.type_key.clone(), idx);
        self.by_type.insert(entry.type_id, idx);
        self.entries.push(entry);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ValueTypeRegistry
// ---------------------------------------------------------------------------

/// Engine-private registry that enforces a strict one-to-one mapping between
/// stable string keys and concrete Rust types.
///
/// `Value` objects hold a `type_idx` — a stable index into the internal
/// `Vec<TypeEntry>` — rather than a heap-allocated key string.  All
/// type-related operations on a `Value` are routed through this registry.
pub(crate) struct ValueTypeRegistry {
    store: std::sync::RwLock<RegistryStore>,
}

impl Default for ValueTypeRegistry {
    fn default() -> Self {
        Self {
            store: std::sync::RwLock::new(RegistryStore::default()),
        }
    }
}

impl ValueTypeRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register a concrete value type `T` under `type_key`.
    pub(crate) fn register<T>(&self, type_key: impl Into<String>) -> Result<(), RegistryError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        self.store
            .write()
            .unwrap()
            .insert(TypeEntry::new::<T>(type_key.into()))
    }

    /// Create a `Value` for a concrete `T`.  Fails if `T` is not registered.
    pub(crate) fn make_value<T>(&self, v: T) -> Result<Value, RegistryError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let store = self.store.read().unwrap();
        let (idx, entry) = store.get_by_type(TypeId::of::<T>()).ok_or_else(|| {
            RegistryError::new(format!(
                "type `{}` is not registered; call \
                 engine.register_value_type::<{0}>() first",
                std::any::type_name::<T>(),
            ))
        })?;
        Ok(entry.make_value(v, idx))
    }

    /// Deserialize raw bytes into a typed `Value`.
    pub(crate) fn deserialize_value(
        &self,
        type_key: &str,
        bytes: &[u8],
    ) -> Result<Value, RegistryError> {
        let store = self.store.read().unwrap();
        let (idx, entry) = store.get_by_key(type_key).ok_or_else(|| {
            RegistryError::new(format!(
                "unknown type key {:?}; register the type with \
                 engine.register_value_type::<T>() before loading",
                type_key
            ))
        })?;
        entry.deserialize(bytes, idx)
    }

    /// Downcast a `Value` to an owned `T`.
    pub(crate) fn downcast_value<T>(&self, value: &Value) -> Result<T, RegistryError>
    where
        T: Any + Clone + 'static,
    {
        let store = self.store.read().unwrap();
        let (idx, entry) = store.get_by_type(TypeId::of::<T>()).ok_or_else(|| {
            RegistryError::new(format!(
                "type mismatch – `<unregistered T>` cannot match value at index {}",
                value.type_idx
            ))
        })?;
        if value.type_idx != idx {
            return Err(RegistryError::new(format!(
                "type mismatch – expected `{}` (idx {}) but value has idx {}",
                entry.type_name, idx, value.type_idx
            )));
        }
        entry.downcast_value::<T>(value)
    }

    /// Return `true` if `type_key` is registered.
    pub(crate) fn contains_key(&self, type_key: &str) -> bool {
        self.store.read().unwrap().get_by_key(type_key).is_some()
    }

    /// Serialise a `Value` to bytes.
    pub(crate) fn serialize_value(&self, value: &Value) -> Vec<u8> {
        self.store
            .read()
            .unwrap()
            .get_by_idx(value.type_idx)
            .serialize(value)
    }

    /// Return the registered string key for a `Value`.
    pub(crate) fn type_key_of<'a>(
        &'a self,
        value: &Value,
    ) -> impl std::ops::Deref<Target = str> + 'a {
        // We need to return a ref into the locked store.  Use a guard-carrying
        // wrapper so the lock is held for the lifetime of the returned value.
        struct KeyGuard<'g>(std::sync::RwLockReadGuard<'g, RegistryStore>, usize);
        impl std::ops::Deref for KeyGuard<'_> {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0.entries[self.1].type_key
            }
        }
        let idx = value.type_idx;
        KeyGuard(self.store.read().unwrap(), idx)
    }

    /// Return the registered type key string for `T`, or `None` if unregistered.
    pub(crate) fn key_for_type_id(&self, tid: TypeId) -> Option<String> {
        self.store
            .read()
            .unwrap()
            .get_by_type(tid)
            .map(|(_, e)| e.type_key.clone())
    }

    /// Register all primitive types and `String` with their canonical keys.
    pub(crate) fn register_primitives(&self) -> Result<(), RegistryError> {
        self.register::<bool>("bool")?;
        self.register::<i8>("i8")?;
        self.register::<i16>("i16")?;
        self.register::<i32>("i32")?;
        self.register::<i64>("i64")?;
        self.register::<i128>("i128")?;
        self.register::<u8>("u8")?;
        self.register::<u16>("u16")?;
        self.register::<u32>("u32")?;
        self.register::<u64>("u64")?;
        self.register::<u128>("u128")?;
        self.register::<f32>("f32")?;
        self.register::<f64>("f64")?;
        self.register::<String>("String")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> ValueTypeRegistry {
        let r = ValueTypeRegistry::new();
        r.register_primitives().unwrap();
        r
    }

    #[test]
    fn make_value_succeeds_for_registered_type() {
        let r = make_registry();
        let v = r.make_value(42u32).unwrap();
        assert_eq!(&*r.type_key_of(&v), "u32");
        assert_eq!(r.downcast_value::<u32>(&v).unwrap(), 42u32);
    }

    #[test]
    fn make_value_fails_for_unregistered_type() {
        let r = make_registry();
        #[derive(Clone, serde::Serialize, serde::Deserialize)]
        struct Custom(i32);
        assert!(r.make_value(Custom(1)).is_err());
    }

    #[test]
    fn register_is_idempotent_for_same_pair() {
        let r = ValueTypeRegistry::new();
        r.register::<i32>("i32").unwrap();
        r.register::<i32>("i32").unwrap();
    }

    #[test]
    fn register_rejects_same_key_different_type() {
        let r = ValueTypeRegistry::new();
        r.register::<i32>("my_key").unwrap();
        assert!(r.register::<i64>("my_key").is_err());
    }

    #[test]
    fn register_rejects_same_type_different_key() {
        let r = ValueTypeRegistry::new();
        r.register::<i32>("key_a").unwrap();
        assert!(r.register::<i32>("key_b").is_err());
    }

    #[test]
    fn deserialize_round_trips_value() {
        let r = make_registry();
        let v = r.make_value(99i64).unwrap();
        let bytes = r.serialize_value(&v);
        let v2 = r.deserialize_value("i64", &bytes).unwrap();
        assert_eq!(r.downcast_value::<i64>(&v2).unwrap(), 99i64);
    }

    #[test]
    fn deserialize_fails_for_unknown_key() {
        let r = make_registry();
        assert!(r.deserialize_value("nonexistent", &[]).is_err());
    }

    #[test]
    fn hash_bytes_is_deterministic() {
        let r = make_registry();
        let v = r.make_value(String::from("hello")).unwrap();
        assert_eq!(
            hash_bytes(&r.serialize_value(&v)),
            hash_bytes(&r.serialize_value(&v))
        );
    }

    #[test]
    fn hash_bytes_differs_for_different_values() {
        let r = make_registry();
        let v1 = r.make_value(1i32).unwrap();
        let v2 = r.make_value(2i32).unwrap();
        assert_ne!(
            hash_bytes(&r.serialize_value(&v1)),
            hash_bytes(&r.serialize_value(&v2))
        );
    }

    #[test]
    fn downcast_value_type_mismatch_returns_error() {
        let r = make_registry();
        let v = r.make_value(42i32).unwrap();
        let result = r.downcast_value::<i64>(&v);
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("type mismatch"));
    }

    #[test]
    fn serialize_is_deterministic() {
        let r = make_registry();
        let v1 = r.make_value(42u64).unwrap();
        let v2 = r.make_value(42u64).unwrap();
        assert_eq!(r.serialize_value(&v1), r.serialize_value(&v2));
    }
}
