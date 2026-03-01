//! Type-erased value container and content-hash helpers.
//!
//! `Value` is an internal implementation detail of the incremental engine.
//! Users of the engine never construct or inspect `Value` directly; all
//! interaction goes through the typed API on [`crate::engine::IncrementalEngine`].
//!
//! ## Design: Type Erasure with Registry-Backed Serde
//!
//! Every `Value` carries a `type_key` that maps it back to a concrete
//! deserializer in the engine-private [`ValueTypeRegistry`].  This makes
//! persistence fully typed: bytes stored on disk always carry their type key,
//! and reload reconstitutes the original concrete value without `Vec<u8>`
//! wrapping.
//!
//! ## Design: Content Hash
//!
//! [`hash_bytes`] applies [`DefaultHasher`] over the serialised bytes.
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

/// A type-erased, cheaply-cloneable, serde-safe node value.
///
/// **This is an internal type.**  Users interact with the engine through the
/// typed APIs (`add_input`, `set_input`, `get_value`) and never hold `Value`
/// directly.
#[derive(Clone)]
pub(crate) struct Value {
    inner: Arc<dyn Any + Send + Sync + 'static>,
    /// Stable registry key that identifies the concrete type.
    type_key: String,
}

impl Value {
    /// Construct a `Value` from a concrete typed value and its registry key.
    ///
    /// This is `pub(crate)`; only [`ValueTypeRegistry`] and engine internals
    /// should call this.
    pub(crate) fn new_with_key<T>(v: T, type_key: impl Into<String>) -> Self
    where
        T: Any + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(v),
            type_key: type_key.into(),
        }
    }

    pub(crate) fn type_key(&self) -> &str {
        &self.type_key
    }

    pub(crate) fn downcast<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Value(type_key={:?})", self.type_key)
    }
}

// ---------------------------------------------------------------------------
// ValueHash
// ---------------------------------------------------------------------------

/// A 64-bit content hash derived from a value's serialised representation.
pub type ValueHash = u64;

/// Compute a [`ValueHash`] by hashing the **serialised bytes** of a value.
///
/// Two values that are logically equal will produce the same bytes (via serde)
/// and therefore the same hash, enabling the early-exit optimisation.
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

/// Internal entry for a single registered concrete type.
struct TypeEntry {
    type_id: TypeId,
    type_name: &'static str,
    type_key: String,
    serialize: fn(&Value) -> Result<Vec<u8>, rmp_serde::encode::Error>,
    deserialize:
        fn(&[u8]) -> Result<Arc<dyn Any + Send + Sync + 'static>, rmp_serde::decode::Error>,
}

impl TypeEntry {
    fn new<T>(type_key: String) -> Self
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            type_key,
            serialize: Self::serialize_fn::<T>,
            deserialize: Self::deserialize_fn::<T>,
        }
    }

    /// Wrap a concrete `T` in a `Value` tagged with this entry's key.
    fn make_value<T: Any + Send + Sync + 'static>(&self, v: T) -> Value {
        Value::new_with_key(v, self.type_key.clone())
    }

    fn serialize_fn<T: Serialize + Any + Send + Sync + 'static>(
        value: &Value,
    ) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        let typed = value
            .downcast::<T>()
            .expect("TypeEntry::serialize_fn: downcast must match registered type");
        rmp_serde::to_vec(typed)
    }

    /// Serialise `value` to bytes, attaching diagnostic context on failure.
    fn serialize(&self, value: &Value) -> Vec<u8> {
        (self.serialize)(value).unwrap_or_else(|e| {
            panic!(
                "TypeEntry::serialize: failed to serialise type `{}` (key {:?}): {}",
                self.type_name, self.type_key, e
            )
        })
    }

    fn deserialize_fn<T: DeserializeOwned + Any + Send + Sync + 'static>(
        bytes: &[u8],
    ) -> Result<Arc<dyn Any + Send + Sync + 'static>, rmp_serde::decode::Error> {
        let v: T = rmp_serde::from_slice(bytes)?;
        Ok(Arc::new(v))
    }

    /// Deserialize `bytes` into a `Value`, attaching diagnostic context on failure.
    fn deserialize(&self, bytes: &[u8]) -> Result<Value, RegistryError> {
        let boxed = (self.deserialize)(bytes).map_err(|e| {
            RegistryError::new(format!(
                "failed to deserialize type `{}` (key {:?}): {}",
                self.type_name, self.type_key, e
            ))
        })?;
        // Downcast the Box<dyn Any> back to T and wrap in Value.
        // SAFETY: deserialize_fn::<T> always boxes a T, so this cast is sound.
        Ok(Value {
            inner: boxed,
            type_key: self.type_key.clone(),
        })
    }

    /// Downcast a `Value` to an owned `T`, with context for error messages.
    fn downcast_value<T: Any + Clone + 'static>(
        &self,
        value: &Value,
        context: &str,
    ) -> Result<T, RegistryError> {
        if value.type_key() != self.type_key.as_str() {
            return Err(RegistryError::new(format!(
                "{}: type mismatch – expected `{}` (key {:?}) but value has key {:?}",
                context,
                self.type_name,
                self.type_key,
                value.type_key()
            )));
        }
        value.downcast::<T>().cloned().ok_or_else(|| {
            RegistryError::new(format!(
                "{}: downcast to `{}` (key {:?}) failed",
                context, self.type_name, self.type_key
            ))
        })
    }
}

/// The inner mutable state of [`ValueTypeRegistry`], held under a single
/// `RwLock`.
///
/// `entries` is the canonical store; `by_key` and `by_type` are integer
/// indices into it.  Because entries are never removed, indices are stable
/// for the lifetime of the registry.
#[derive(Default)]
struct RegistryStore {
    entries: Vec<TypeEntry>,
    by_key: std::collections::HashMap<String, usize>,
    by_type: std::collections::HashMap<TypeId, usize>,
}

impl RegistryStore {
    fn get_by_key(&self, key: &str) -> Option<&TypeEntry> {
        self.by_key.get(key).map(|&i| &self.entries[i])
    }
    fn get_by_type(&self, tid: TypeId) -> Option<&TypeEntry> {
        self.by_type.get(&tid).map(|&i| &self.entries[i])
    }

    /// Insert `entry` if no conflict exists, otherwise validate idempotency.
    ///
    /// - Same `(type_id, type_key)` already present → no-op, `Ok(())`.
    /// - `type_id` mapped to a different key, or key mapped to a different
    ///   type → `Err`.
    /// - Not present → push to `entries` and update both indices.
    fn insert(&mut self, entry: TypeEntry) -> Result<(), RegistryError> {
        if let Some(existing) = self.get_by_type(entry.type_id) {
            if existing.type_key != entry.type_key {
                return Err(RegistryError::new(format!(
                    "type `{}` is already registered under key {:?}, \
                     cannot re-register under {:?}",
                    entry.type_name, existing.type_key, entry.type_key
                )));
            }
            return Ok(()); // idempotent
        }
        if let Some(existing) = self.get_by_key(&entry.type_key) {
            if existing.type_id != entry.type_id {
                return Err(RegistryError::new(format!(
                    "key {:?} is already registered for type `{}`, \
                     cannot re-register for type `{}`",
                    entry.type_key, existing.type_name, entry.type_name
                )));
            }
            return Ok(()); // idempotent
        }
        let idx = self.entries.len();
        self.by_key.insert(entry.type_key.clone(), idx);
        self.by_type.insert(entry.type_id, idx);
        self.entries.push(entry);
        Ok(())
    }
}

/// Engine-private registry that enforces a strict one-to-one mapping between
/// stable string keys and concrete Rust types.
///
/// # One-to-one invariant
/// - Same `(T, key)` pair: idempotent.
/// - Different `T` for same key: error.
/// - Different key for same `T`: error.
///
/// Internally, [`TypeEntry`] objects live in a `Vec` (the canonical store).
/// Two `HashMap<_, usize>` indices map from key string / `TypeId` to the
/// entry's position in that vec.  This means:
/// - Read-hot paths (serialize, deserialize, downcast) do one lock
///   acquisition and one bounds-checked vec index — no extra indirection.
/// - Registration (write path) is infrequent and holds the single lock for
///   the entire insert.
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
        self.store.write().unwrap().insert(TypeEntry::new::<T>(type_key.into()))
    }

    /// Create a `Value` for a concrete `T`.  Fails if `T` is not registered.
    pub(crate) fn make_value<T>(&self, v: T) -> Result<Value, RegistryError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let store = self.store.read().unwrap();
        let entry = store.get_by_type(TypeId::of::<T>()).ok_or_else(|| {
            RegistryError::new(format!(
                "type `{}` is not registered; call \
                 engine.register_value_type::<{0}>() first",
                std::any::type_name::<T>(),
            ))
        })?;
        Ok(entry.make_value(v))
    }

    /// Deserialize raw bytes into a typed `Value` using the registered deserializer for `type_key`.
    pub(crate) fn deserialize_value(
        &self,
        type_key: &str,
        bytes: &[u8],
    ) -> Result<Value, RegistryError> {
        let store = self.store.read().unwrap();
        let entry = store.get_by_key(type_key).ok_or_else(|| {
            RegistryError::new(format!(
                "unknown type key {:?}; register the type with \
                 engine.register_value_type::<T>() before loading",
                type_key
            ))
        })?;
        entry.deserialize(bytes)
    }

    /// Downcast a `Value` to an owned `T`.
    pub(crate) fn downcast_value<T>(&self, value: &Value, context: &str) -> Result<T, RegistryError>
    where
        T: Any + Clone + 'static,
    {
        let store = self.store.read().unwrap();
        match store.get_by_type(TypeId::of::<T>()) {
            None => Err(RegistryError::new(format!(
                "{}: type mismatch – expected type key \"<unregistered>\" but value has {:?}",
                context,
                value.type_key()
            ))),
            Some(entry) => entry.downcast_value::<T>(value, context),
        }
    }

    /// Return `true` if `type_key` is registered.
    pub(crate) fn contains_key(&self, type_key: &str) -> bool {
        self.store.read().unwrap().get_by_key(type_key).is_some()
    }

    /// Serialise a `Value` to bytes using its registered type's serde impl.
    pub(crate) fn serialize_value(&self, value: &Value) -> Vec<u8> {
        let store = self.store.read().unwrap();
        let entry = store
            .get_by_key(value.type_key())
            .expect("serialize_value: type key not registered; this is a bug");
        entry.serialize(value)
    }

    /// Return the registered type key for `T`, or `None` if unregistered.
    pub(crate) fn key_for_type_id(&self, tid: TypeId) -> Option<String> {
        self.store
            .read()
            .unwrap()
            .get_by_type(tid)
            .map(|e| e.type_key.clone())
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
        assert_eq!(v.type_key(), "u32");
        assert_eq!(v.downcast::<u32>(), Some(&42u32));
    }

    #[test]
    fn make_value_fails_for_unregistered_type() {
        let r = make_registry();
        #[derive(Clone, serde::Serialize, serde::Deserialize)]
        struct Custom(i32);
        let result = r.make_value(Custom(1));
        assert!(result.is_err());
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
        let result = r.register::<i64>("my_key");
        assert!(result.is_err());
    }

    #[test]
    fn register_rejects_same_type_different_key() {
        let r = ValueTypeRegistry::new();
        r.register::<i32>("key_a").unwrap();
        let result = r.register::<i32>("key_b");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_round_trips_value() {
        let r = make_registry();
        let v = r.make_value(99i64).unwrap();
        let bytes = r.serialize_value(&v);
        let v2 = r.deserialize_value("i64", &bytes).unwrap();
        assert_eq!(v2.downcast::<i64>(), Some(&99i64));
    }

    #[test]
    fn deserialize_fails_for_unknown_key() {
        let r = make_registry();
        let result = r.deserialize_value("nonexistent", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn hash_bytes_is_deterministic() {
        let r = make_registry();
        let v = r.make_value(String::from("hello")).unwrap();
        let h1 = hash_bytes(&r.serialize_value(&v));
        let h2 = hash_bytes(&r.serialize_value(&v));
        assert_eq!(h1, h2);
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
        let result = r.downcast_value::<i64>(&v, "test");
        assert!(result.is_err());
        let msg = result.unwrap_err().message;
        assert!(
            msg.contains("type mismatch"),
            "expected type mismatch in: {msg}"
        );
    }

    #[test]
    fn serialize_is_deterministic() {
        let r = make_registry();
        let v1 = r.make_value(42u64).unwrap();
        let v2 = r.make_value(42u64).unwrap();
        assert_eq!(r.serialize_value(&v1), r.serialize_value(&v2));
    }
}
