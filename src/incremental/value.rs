//! Type-erased value container and content-hash helpers.
//!
//! ## Design: Type Erasure with Serde Safety
//!
//! The incremental graph must store heterogeneous node values – e.g. a token
//! stream, an AST, a diagnostic list – without making the `Graph` struct
//! generic over every possible value type.  We achieve this via
//! `Arc<dyn Any + Send + Sync>`.
//!
//! **Key constraint**: every value stored in the graph **must be serialisable
//! and deserialisable** using `serde`.  This is enforced at the [`Value::new`]
//! call site by the `T: Serialize + DeserializeOwned` bound.  The concrete
//! type's `Serialize` impl is captured at construction time as a
//! `Box<dyn Fn() -> Vec<u8>>` closure, allowing the rest of the system to
//! serialise the value to MessagePack bytes without knowing its concrete type.
//! Similarly, a deserialise function `Box<dyn Fn(&[u8]) -> Result<Arc<dyn Any + …>>>>`
//! is captured so the loader can reconstruct the typed value from bytes.
//!
//! This design satisfies three requirements simultaneously:
//! - **Heterogeneous storage** – `Graph` stays non-generic.
//! - **Cheap cloning** – `Arc` makes clone O(1).
//! - **Safe persistence** – bytes are always a valid serde round-trip, not a
//!   raw memory dump.
//!
//! ## Design: Content Hash
//!
//! We use [`std::hash::DefaultHasher`] (fast, non-cryptographic) over the
//! serialised bytes.  Hashing the canonical byte representation (rather than
//! the in-memory pointer) means two logically-equal values produced by
//! separate transform invocations will produce the same hash and trigger the
//! early-exit optimisation.

use std::any::Any;
use std::hash::{Hash, Hasher, DefaultHasher};
use std::sync::Arc;
use std::fmt;
use serde::{Serialize, de::DeserializeOwned};

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

/// Type alias for the serialise closure stored inside every [`Value`].
type SerializeFn = Arc<dyn Fn() -> Vec<u8> + Send + Sync + 'static>;

/// Type alias for the deserialise closure stored inside every [`Value`].
type DeserializeFn = Arc<dyn Fn(&[u8]) -> Result<Arc<dyn Any + Send + Sync + 'static>, String> + Send + Sync + 'static>;

/// A type-erased, cheaply-cloneable, serde-safe node value.
///
/// The concrete type `T` must implement [`Serialize`] and [`DeserializeOwned`]
/// in addition to [`Any`] + [`Send`] + [`Sync`].  These bounds are checked
/// **at construction time** ([`Value::new`]); once wrapped, the value can be
/// passed around type-erased without carrying the bounds in every signature.
///
/// # Serialisation
///
/// Call [`Value::to_bytes`] to encode to MessagePack at any time.  Call
/// [`Value::from_bytes`] with the *same concrete type* to reconstruct.
#[derive(Clone)]
pub struct Value {
    inner: Arc<dyn Any + Send + Sync + 'static>,
    serialize_fn: SerializeFn,
    deserialize_fn: DeserializeFn,
}

impl Value {
    /// Wrap a concrete value.
    ///
    /// # Type Constraints
    ///
    /// `T` must implement:
    /// - `Any + Send + Sync + 'static` – for type-erased runtime storage.
    /// - `Serialize` – so the value can be written to the key-value store.
    /// - `DeserializeOwned` – so a stored value can be reconstructed on load.
    /// - `Clone` – the serialise closure needs to capture `T` by value.
    pub fn new<T>(v: T) -> Self
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let arc: Arc<dyn Any + Send + Sync + 'static> = Arc::new(v.clone());

        // Capture a clone for the serialise closure.
        let v_for_ser = v;
        let serialize_fn: SerializeFn = Arc::new(move || {
            rmp_serde::to_vec(&v_for_ser)
                .expect("Value::serialize_fn: serde serialisation must not fail for a valid T")
        });

        // The deserialise closure decodes bytes → Arc<dyn Any>.
        let deserialize_fn: DeserializeFn = Arc::new(|bytes: &[u8]| {
            let value: T = rmp_serde::from_slice(bytes)
                .map_err(|e| e.to_string())?;
            Ok(Arc::new(value) as Arc<dyn Any + Send + Sync + 'static>)
        });

        Self { inner: arc, serialize_fn, deserialize_fn }
    }

    /// Try to downcast to `&T`.  Returns `None` if the stored type is not `T`.
    pub fn downcast<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }

    /// Return the inner `Arc<dyn Any + Send + Sync>` for callers that need to
    /// move it into tasks.
    pub fn inner(&self) -> Arc<dyn Any + Send + Sync + 'static> {
        Arc::clone(&self.inner)
    }

    /// Encode this value to MessagePack bytes using the captured `Serialize`
    /// implementation.
    ///
    /// The returned bytes are always a valid `rmp-serde` encoding of the
    /// concrete type; they can be restored with [`Value::from_bytes`].
    pub fn to_bytes(&self) -> Vec<u8> {
        (self.serialize_fn)()
    }

    /// Reconstruct a [`Value`] from MessagePack bytes, given the concrete type
    /// `T` used at construction time.
    ///
    /// # Errors
    ///
    /// Returns an error string if `bytes` is not a valid `rmp-serde` encoding
    /// of `T`.
    pub fn from_bytes<T>(bytes: &[u8]) -> Result<Self, String>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let value: T = rmp_serde::from_slice(bytes).map_err(|e| e.to_string())?;
        Ok(Self::new(value))
    }

    /// Reconstruct a [`Value`] from bytes using the deserialise closure that
    /// was captured at construction time (type-erased reconstruction).
    ///
    /// The result contains the typed `Arc<dyn Any>` but retains a new
    /// `serialize_fn` / `deserialize_fn` pair only if the round-trip succeeds.
    ///
    /// **Note**: because the concrete type is not known at this call site,
    /// the reconstructed `Value` only carries the `Arc<dyn Any>` and the
    /// original closures are **not** re-captured (they were built from a
    /// different instance).  For full reconstruction with working closures,
    /// use [`Value::from_bytes::<T>`] when the concrete type is known.
    pub fn reconstruct_from_bytes(&self, bytes: &[u8]) -> Result<Arc<dyn Any + Send + Sync + 'static>, String> {
        (self.deserialize_fn)(bytes)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Value(<dyn Any+Serialize>)")
    }
}

// ---------------------------------------------------------------------------
// ValueHash
// ---------------------------------------------------------------------------

/// A 64-bit content hash derived from a value's serialised representation.
///
/// Hashing the canonical serialised bytes (rather than the in-memory pointer)
/// means two logically-equal values produced by independent transform
/// invocations will hash identically and trigger the early-exit optimisation.
pub type ValueHash = u64;

/// Compute a [`ValueHash`] for any `T: Hash`.
///
/// Uses [`DefaultHasher`].  The hash is only stable within a single process
/// lifetime; do not persist it to the key-value store.
pub fn hash_value<T: Hash>(v: &T) -> ValueHash {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Compute a [`ValueHash`] by hashing the **serialised bytes** of a value.
///
/// Preferred over [`hash_value`] for [`Value`] objects, because two values
/// that are logically equal but stored in separate `Arc` allocations will
/// produce the same hash.
pub fn hash_bytes(bytes: &[u8]) -> ValueHash {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

// ---------------------------------------------------------------------------
// HashedValue
// ---------------------------------------------------------------------------

/// A [`Value`] paired with its content hash (derived from serialised bytes).
///
/// Transforms that produce hashable outputs should return a `HashedValue` so
/// the scheduler can perform the early-exit optimisation.
pub struct HashedValue {
    pub value: Value,
    pub hash: ValueHash,
}

impl HashedValue {
    /// Wrap `v`, serialise it, and compute its hash in one call.
    pub fn new<T>(v: T) -> Self
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let value = Value::new(v);
        let bytes = value.to_bytes();
        let hash = hash_bytes(&bytes);
        Self { value, hash }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downcast_succeeds_for_correct_type() {
        let v = Value::new(42u32);
        assert_eq!(v.downcast::<u32>(), Some(&42u32));
    }

    #[test]
    fn downcast_fails_for_wrong_type() {
        let v = Value::new(42u32);
        assert!(v.downcast::<i32>().is_none());
    }

    #[test]
    fn clone_shares_allocation() {
        let v = Value::new(String::from("hello"));
        let v2 = v.clone();
        // Both arcs point to same allocation.
        assert!(Arc::ptr_eq(&v.inner(), &v2.inner()));
    }

    #[test]
    fn to_bytes_round_trips() {
        let v = Value::new(123i32);
        let bytes = v.to_bytes();
        let v2 = Value::from_bytes::<i32>(&bytes).unwrap();
        assert_eq!(v2.downcast::<i32>(), Some(&123i32));
    }

    #[test]
    fn to_bytes_is_deterministic() {
        let v1 = Value::new(42u64);
        let v2 = Value::new(42u64);
        assert_eq!(v1.to_bytes(), v2.to_bytes());
    }

    #[test]
    fn different_values_produce_different_bytes() {
        let v1 = Value::new(1u64);
        let v2 = Value::new(2u64);
        assert_ne!(v1.to_bytes(), v2.to_bytes());
    }

    #[test]
    fn hash_bytes_is_deterministic() {
        let v = Value::new(String::from("hello world"));
        let h1 = hash_bytes(&v.to_bytes());
        let h2 = hash_bytes(&v.to_bytes());
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_value_is_deterministic() {
        let h1 = hash_value(&"hello world");
        let h2 = hash_value(&"hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_value_differs_for_different_inputs() {
        let h1 = hash_value(&"foo");
        let h2 = hash_value(&"bar");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hashed_value_wraps_and_hashes() {
        let hv = HashedValue::new(100u64);
        assert_eq!(hv.value.downcast::<u64>(), Some(&100u64));
        // Hash is derived from bytes, must be consistent.
        let expected = hash_bytes(&hv.value.to_bytes());
        assert_eq!(hv.hash, expected);
    }

    #[test]
    fn from_bytes_error_on_wrong_type() {
        let v = Value::new(42u32);
        let bytes = v.to_bytes();
        // Try to decode as String – should fail gracefully.
        let result = Value::from_bytes::<String>(&bytes);
        assert!(result.is_err());
    }
}