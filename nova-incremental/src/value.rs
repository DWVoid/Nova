//! Type-erased value container and content-hash helpers.
//!
//! `Value` is an internal implementation detail of the incremental engine.
//! Users of the engine never construct or inspect `Value` directly; all
//! interaction goes through the typed API on [`crate::engine::IncrementalEngine`].
//!
//! ## Design: Type Erasure with Registry-Backed Serde
//!
//! Every `Value` carries a `type_key` that maps it back to a concrete
//! deserialiser in the engine-private [`ValueTypeRegistry`].  This makes
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

use std::any::{Any, TypeId};
use std::hash::{Hash, Hasher, DefaultHasher};
use std::sync::Arc;
use std::fmt;
use serde::{Serialize, de::DeserializeOwned};

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

/// A type-erased, cheaply-cloneable, serde-safe node value.
///
/// **This is an internal type.**  Users interact with the engine through the
/// typed APIs (`add_input`, `set_input`, `get_value`) and never hold `Value`
/// directly.
///
/// Serialization is handled by a plain `fn` pointer monomorphized per `T`
/// at construction time – no heap allocation, no captured clone of the value.
#[derive(Clone)]
pub(crate) struct Value {
    inner: Arc<dyn Any + Send + Sync + 'static>,
    /// Stable registry key that identifies the concrete type.
    type_key: String,
    /// Monomorphized serializer: receives the `Arc`'s data as `&dyn Any`
    /// and calls `rmp_serde::to_vec` on the correctly-typed reference.
    serialize_fn: fn(&(dyn Any + Send + Sync + 'static)) -> Vec<u8>,
}

impl Value {
    /// Construct a `Value` from a concrete typed value and its registry key.
    ///
    /// This is `pub(crate)`; only [`ValueTypeRegistry`] and engine internals
    /// should call this.
    pub(crate) fn new_with_key<T>(v: T, type_key: impl Into<String>) -> Self
    where
        T: Any + Send + Sync + Serialize + DeserializeOwned + 'static,
    {
        fn serialize<T: Serialize + Any + Send + Sync + 'static>(
            any: &(dyn Any + Send + Sync + 'static),
        ) -> Vec<u8> {
            let typed: &T = any.downcast_ref::<T>()
                .expect("Value::serialize_fn: downcast must match construction type");
            rmp_serde::to_vec(typed)
                .expect("Value::serialize_fn: serialisation must not fail for a valid T")
        }
        Self {
            inner: Arc::new(v),
            type_key: type_key.into(),
            serialize_fn: serialize::<T>,
        }
    }

    pub(crate) fn type_key(&self) -> &str { &self.type_key }

    pub(crate) fn downcast<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        (self.serialize_fn)(self.inner.as_ref())
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
        Self { message: msg.into() }
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
    deserialize: Arc<dyn Fn(&[u8]) -> Result<Value, RegistryError> + Send + Sync + 'static>,
}

/// Engine-private registry that enforces a strict one-to-one mapping between
/// stable string keys and concrete Rust types.
///
/// # One-to-one invariant
/// - Same `(T, key)` pair: idempotent.
/// - Different `T` for same key: error.
/// - Different key for same `T`: error.
///
/// Uses `RwLock` internally so `register` takes `&self`, allowing the registry
/// to live behind an `Arc` that is shared with the loader and transforms while
/// still accepting new type registrations.
pub(crate) struct ValueTypeRegistry {
    by_key: std::sync::RwLock<std::collections::HashMap<String, TypeEntry>>,
    by_type: std::sync::RwLock<std::collections::HashMap<TypeId, String>>,
}

impl Default for ValueTypeRegistry {
    fn default() -> Self {
        Self {
            by_key: std::sync::RwLock::new(std::collections::HashMap::new()),
            by_type: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl ValueTypeRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register a concrete value type `T` under `type_key`.
    ///
    /// Takes `&self` (interior mutability via `RwLock`) so the registry can be
    /// shared via `Arc` without needing exclusive ownership.
    pub(crate) fn register<T>(&self, type_key: impl Into<String>) -> Result<(), RegistryError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let key = type_key.into();
        let tid = TypeId::of::<T>();
        let tname = std::any::type_name::<T>();

        // T already mapped to a key?
        if let Some(existing_key) = self.by_type.read().unwrap().get(&tid) {
            if *existing_key != key {
                return Err(RegistryError::new(format!(
                    "type `{}` is already registered under key {:?}, \
                     cannot re-register under {:?}",
                    tname, existing_key, key
                )));
            }
            return Ok(()); // idempotent
        }

        // Key already mapped to a different type?
        if let Some(existing) = self.by_key.read().unwrap().get(&key) {
            if existing.type_id != tid {
                return Err(RegistryError::new(format!(
                    "key {:?} is already registered for type `{}`, \
                     cannot re-register for type `{}`",
                    key, existing.type_name, tname
                )));
            }
            return Ok(()); // idempotent
        }

        let key_clone = key.clone();
        let deserialize: Arc<dyn Fn(&[u8]) -> Result<Value, RegistryError> + Send + Sync + 'static> =
            Arc::new(move |bytes: &[u8]| {
                let v: T = rmp_serde::from_slice(bytes).map_err(|e| {
                    RegistryError::new(format!(
                        "failed to deserialise type `{}` from bytes: {}", tname, e
                    ))
                })?;
                Ok(Value::new_with_key(v, key_clone.clone()))
            });

        self.by_key.write().unwrap().insert(key.clone(), TypeEntry {
            type_id: tid,
            type_name: tname,
            deserialize,
        });
        self.by_type.write().unwrap().insert(tid, key);
        Ok(())
    }

    /// Create a `Value` for a concrete `T`.  Fails if `T` is not registered.
    pub(crate) fn make_value<T>(&self, v: T) -> Result<Value, RegistryError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let tid = TypeId::of::<T>();
        let key = self.by_type.read().unwrap().get(&tid).cloned().ok_or_else(|| {
            RegistryError::new(format!(
                "type `{}` is not registered; call \
                 engine.register_value_type::<{0}>() first",
                std::any::type_name::<T>(),
            ))
        })?;
        Ok(Value::new_with_key(v, key))
    }

    /// Deserialise raw bytes into a typed `Value` using the registered
    /// deserialiser for `type_key`.
    pub(crate) fn deserialize_value(&self, type_key: &str, bytes: &[u8]) -> Result<Value, RegistryError> {
        let deserialize_fn = {
            let guard = self.by_key.read().unwrap();
            let entry = guard.get(type_key).ok_or_else(|| {
                RegistryError::new(format!(
                    "unknown type key {:?}; register the type with \
                     engine.register_value_type::<T>() before loading",
                    type_key
                ))
            })?;
            Arc::clone(&entry.deserialize)
        };
        (deserialize_fn)(bytes)
    }

    /// Downcast a `Value` to an owned `T`, returning an error with context if
    /// the type key or runtime type does not match.
    pub(crate) fn downcast_value<T>(&self, value: &Value, context: &str) -> Result<T, RegistryError>
    where
        T: Any + Clone + 'static,
    {
        let expected_key = self.by_type.read().unwrap()
            .get(&TypeId::of::<T>())
            .cloned()
            .unwrap_or_else(|| "<unregistered>".to_string());

        if value.type_key() != expected_key.as_str() {
            return Err(RegistryError::new(format!(
                "{}: type mismatch – expected type key {:?} but value has {:?}",
                context, expected_key, value.type_key()
            )));
        }
        value.downcast::<T>().cloned().ok_or_else(|| {
            RegistryError::new(format!(
                "{}: downcast failed for type key {:?}",
                context, value.type_key()
            ))
        })
    }

    /// Return `true` if `type_key` is registered.
    pub(crate) fn contains_key(&self, type_key: &str) -> bool {
        self.by_key.read().unwrap().contains_key(type_key)
    }

    /// Return the registered type key for `T`, or `None` if unregistered.
    pub(crate) fn key_for_type_id(&self, tid: TypeId) -> Option<String> {
        self.by_type.read().unwrap().get(&tid).cloned()
    }

    /// Register all primitive types and `String` with their canonical keys.
    /// Called unconditionally by [`IncrementalEngine::new`] and
    /// [`IncrementalEngine::load`].
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
        let mut r = ValueTypeRegistry::new();
        r.register::<i32>("i32").unwrap();
        r.register::<i32>("i32").unwrap();
    }

    #[test]
    fn register_rejects_same_key_different_type() {
        let mut r = ValueTypeRegistry::new();
        r.register::<i32>("my_key").unwrap();
        let result = r.register::<i64>("my_key");
        assert!(result.is_err());
    }

    #[test]
    fn register_rejects_same_type_different_key() {
        let mut r = ValueTypeRegistry::new();
        r.register::<i32>("key_a").unwrap();
        let result = r.register::<i32>("key_b");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_round_trips_value() {
        let r = make_registry();
        let v = r.make_value(99i64).unwrap();
        let bytes = v.to_bytes();
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
        let h1 = hash_bytes(&v.to_bytes());
        let h2 = hash_bytes(&v.to_bytes());
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_bytes_differs_for_different_values() {
        let r = make_registry();
        let v1 = r.make_value(1i32).unwrap();
        let v2 = r.make_value(2i32).unwrap();
        assert_ne!(hash_bytes(&v1.to_bytes()), hash_bytes(&v2.to_bytes()));
    }

    #[test]
    fn downcast_value_type_mismatch_returns_error() {
        let r = make_registry();
        let v = r.make_value(42i32).unwrap();
        let result = r.downcast_value::<i64>(&v, "test");
        assert!(result.is_err());
        let msg = result.unwrap_err().message;
        assert!(msg.contains("type mismatch"), "expected type mismatch in: {msg}");
    }

    #[test]
    fn to_bytes_is_deterministic() {
        let r = make_registry();
        let v1 = r.make_value(42u64).unwrap();
        let v2 = r.make_value(42u64).unwrap();
        assert_eq!(v1.to_bytes(), v2.to_bytes());
    }
}
