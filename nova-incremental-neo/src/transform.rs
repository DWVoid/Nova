//! Public [`Transform`] trait, [`TransformContext`], and supporting types.
//!
//! ## What external crates see
//! - [`Transform`] — the trait to implement.
//! - [`TransformContext`] — typed I/O passed to [`Transform::apply`].
//! - [`TransformRegisterContext`] — slot-declaration DSL (sealed, not externally implementable).
//! - [`TransformError`], [`IncrementalValue`], [`KeyExtractor`],
//!   [`CollectionInput`], [`CollectionOutputBuilder`].
//!
//! All internal machinery is `pub(crate)` only.

use std::any::{Any, TypeId};
use std::sync::Arc;
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

// ---------------------------------------------------------------------------
// IncrementalValue
// ---------------------------------------------------------------------------

/// Marker trait auto-implemented for any type that can flow through the graph.
pub trait IncrementalValue:
    Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static
{}

impl<T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static>
    IncrementalValue for T {}

// ---------------------------------------------------------------------------
// KeyExtractor
// ---------------------------------------------------------------------------

/// Derives a stable `u64` identity key from a collection element.
///
/// Implement on a zero-sized marker struct. Keys must be:
/// - **Stable**: same logical element → same key across sessions.
/// - **Unique** within a collection.
/// - **Pure**: no side effects.
pub trait KeyExtractor<T>: Send + Sync + 'static {
    fn extract_key(item: &T) -> u64;
}

// ---------------------------------------------------------------------------
// TransformError
// ---------------------------------------------------------------------------

/// Error produced during transform execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransformError {
    pub message: String,
    pub source: Option<String>,
}

impl TransformError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), source: None }
    }
    pub fn with_source(message: impl Into<String>, source: impl Into<String>) -> Self {
        Self { message: message.into(), source: Some(source.into()) }
    }
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(s) = &self.source { write!(f, ": {s}")?; }
        Ok(())
    }
}
impl std::error::Error for TransformError {}

// ---------------------------------------------------------------------------
// CollectionInput — typed facade for reading collection inputs
// ---------------------------------------------------------------------------

use std::collections::HashMap;

/// Owned typed facade for a gathered collection input slot.
///
/// Provides key-based access to elements and exposes incremental diff information.
/// The transform can query elements by key, iterate over all elements, or
/// iterate only over added/changed elements.
pub struct CollectionInput<T> {
    /// All current elements, keyed by their extracted key.
    elements_by_key: HashMap<u64, T>,
    /// All current keys in sorted order.
    current_keys: Vec<u64>,
    /// Keys of elements that were added since last evaluation.
    added_keys: Vec<u64>,
    /// Keys of elements whose value changed since last evaluation.
    changed_keys: Vec<u64>,
    /// Keys of elements that were removed since last evaluation.
    removed_keys: Vec<u64>,
}

impl<T> CollectionInput<T> {
    /// Create a new CollectionInput from the given data.
    pub(crate) fn new(
        elements_by_key: HashMap<u64, T>,
        current_keys: Vec<u64>,
        added_keys: Vec<u64>,
        changed_keys: Vec<u64>,
        removed_keys: Vec<u64>,
    ) -> Self {
        Self { elements_by_key, current_keys, added_keys, changed_keys, removed_keys }
    }

    /// Get a reference to an element by key.
    pub fn get(&self, key: u64) -> Option<&T> {
        self.elements_by_key.get(&key)
    }

    /// Check if an element with the given key exists.
    pub fn contains(&self, key: u64) -> bool {
        self.elements_by_key.contains_key(&key)
    }

    /// Get the number of elements in the collection.
    pub fn len(&self) -> usize {
        self.elements_by_key.len()
    }

    /// Check if the collection is empty.
    pub fn is_empty(&self) -> bool {
        self.elements_by_key.is_empty()
    }

    /// Get all current keys in the collection (sorted).
    pub fn keys(&self) -> &[u64] {
        &self.current_keys
    }

    /// Iterate over all (key, element) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &T)> {
        self.current_keys.iter().filter_map(move |&k| {
            self.elements_by_key.get(&k).map(|v| (k, v))
        })
    }

    /// Iterate over all elements (without keys).
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.current_keys.iter().filter_map(move |&k| self.elements_by_key.get(&k))
    }

    /// Get keys of elements that were added since last evaluation.
    pub fn added_keys(&self) -> &[u64] {
        &self.added_keys
    }

    /// Get keys of elements whose value changed since last evaluation.
    pub fn changed_keys(&self) -> &[u64] {
        &self.changed_keys
    }

    /// Get keys of elements that were removed since last evaluation.
    pub fn removed_keys(&self) -> &[u64] {
        &self.removed_keys
    }

    /// Iterate over elements that were added since last evaluation.
    pub fn added(&self) -> impl Iterator<Item = &T> {
        self.added_keys.iter().filter_map(move |&k| self.elements_by_key.get(&k))
    }

    /// Iterate over elements that changed since last evaluation.
    pub fn changed(&self) -> impl Iterator<Item = &T> {
        self.changed_keys.iter().filter_map(move |&k| self.elements_by_key.get(&k))
    }

    /// Check if this collection has any changes (added, changed, or removed).
    pub fn has_changes(&self) -> bool {
        !self.added_keys.is_empty() || !self.changed_keys.is_empty() || !self.removed_keys.is_empty()
    }
}

// ---------------------------------------------------------------------------
// CollectionOutputBuilder — typed facade for mutating collection outputs
// ---------------------------------------------------------------------------

/// Builder for incrementally mutating a collection output slot.
///
/// Allows the transform to add, set, or remove individual elements without
/// rebuilding the entire collection. The engine tracks mutations and computes
/// the actual diff during commit.
pub struct CollectionOutputBuilder<'a, T: IncrementalValue> {
    slot: usize,
    ctx: &'a mut TransformContext,
    _phantom: std::marker::PhantomData<T>,
}

impl<'a, T: IncrementalValue> CollectionOutputBuilder<'a, T> {
    pub(crate) fn new(slot: usize, ctx: &'a mut TransformContext) -> Self {
        Self { slot, ctx, _phantom: std::marker::PhantomData }
    }

    /// Add a new element to the collection.
    /// The key is extracted via the registered KeyExtractor.
    pub fn add(&mut self, element: T) -> Result<(), TransformError> {
        self.ctx.add_collection_element::<T>(self.slot, element)
    }

    /// Set or replace an element with a specific key.
    /// Use this when you know the key and want to ensure a specific value.
    pub fn set(&mut self, key: u64, element: T) -> Result<(), TransformError> {
        self.ctx.set_collection_element::<T>(self.slot, key, element)
    }

    /// Remove an element by key.
    /// If the element doesn't exist, this is a no-op.
    pub fn remove(&mut self, key: u64) -> Result<(), TransformError> {
        self.ctx.remove_collection_element(self.slot, key)
    }

    /// Clear all elements from the collection.
    pub fn clear(&mut self) -> Result<(), TransformError> {
        self.ctx.clear_collection(self.slot)
    }
}

// ---------------------------------------------------------------------------
// SlotKind — internal slot descriptor
// ---------------------------------------------------------------------------

/// Function type for deserializing bytes into an ErasedValue.
pub(crate) type DeserializeFn = fn(&[u8]) -> Result<ErasedValue, String>;

/// Minimal type descriptor for one slot. `pub(crate)` only.
#[derive(Clone, Debug)]
pub(crate) struct SlotKind {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) is_collection: bool,
    /// For collection output slots: extracts a stable `u64` key from a boxed value.
    pub(crate) extract_key: Option<fn(&dyn Any) -> u64>,
    /// Deserialize bytes into ErasedValue for this slot's type.
    pub(crate) deserialize: DeserializeFn,
}

// ---------------------------------------------------------------------------
// TransformRegisterContext — sealed public trait
// ---------------------------------------------------------------------------

mod sealed { pub trait Sealed {} }

/// DSL for declaring a transform's input/output slot layout.
///
/// Passed to [`Transform::register`] once at build time.
/// This trait is **sealed** — it cannot be implemented outside this crate.
pub trait TransformRegisterContext: sealed::Sealed {
    /// Declare a single-value input slot of type `T`.
    fn input<T: IncrementalValue>(&mut self);
    /// Declare a collection input slot (gather) of type `T`, keyed by `K`.
    fn input_collection<T: IncrementalValue, K: KeyExtractor<T>>(&mut self);
    /// Declare a single-value output slot of type `T`.
    fn output<T: IncrementalValue>(&mut self);
    /// Declare a collection output slot (spread) of type `T`, keyed by `K`.
    fn output_collection<T: IncrementalValue, K: KeyExtractor<T>>(&mut self);
}

/// Internal accumulator implementing [`TransformRegisterContext`].
pub(crate) struct SlotRegistrar {
    pub(crate) inputs: Vec<SlotKind>,
    pub(crate) outputs: Vec<SlotKind>,
}

impl SlotRegistrar {
    pub(crate) fn new() -> Self {
        Self { inputs: vec![], outputs: vec![] }
    }
}

impl sealed::Sealed for SlotRegistrar {}

impl TransformRegisterContext for SlotRegistrar {
    fn input<T: IncrementalValue>(&mut self) {
        self.inputs.push(SlotKind {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            is_collection: false,
            extract_key: None,
            deserialize: make_deserialize_fn::<T>(),
        });
    }
    fn input_collection<T: IncrementalValue, K: KeyExtractor<T>>(&mut self) {
        self.inputs.push(SlotKind {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            is_collection: true,
            extract_key: None,
            deserialize: make_deserialize_fn::<T>(),
        });
    }
    fn output<T: IncrementalValue>(&mut self) {
        self.outputs.push(SlotKind {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            is_collection: false,
            extract_key: None,
            deserialize: make_deserialize_fn::<T>(),
        });
    }
    fn output_collection<T: IncrementalValue, K: KeyExtractor<T>>(&mut self) {
        self.outputs.push(SlotKind {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            is_collection: true,
            extract_key: Some(|any: &dyn Any| -> u64 {
                let t = any.downcast_ref::<T>()
                    .expect("KeyExtractor: type mismatch at runtime");
                K::extract_key(t)
            }),
            deserialize: make_deserialize_fn::<T>(),
        });
    }
}

// ---------------------------------------------------------------------------
// SerializableValue trait — object-safe serialization
// ---------------------------------------------------------------------------

/// Object-safe trait for values that can be serialized to bytes.
/// All `IncrementalValue` types automatically implement this via blanket impl.
pub trait SerializableValue: Any + Send + Sync {
    /// Downcast to `dyn Any` for type checking.
    fn as_any(&self) -> &dyn Any;
    /// Serialize this value to msgpack bytes.
    fn to_bytes(&self) -> Result<Vec<u8>, String>;
    /// Get the type name for this value.
    fn type_name(&self) -> &'static str;
}

impl<T: IncrementalValue> SerializableValue for T {
    fn as_any(&self) -> &dyn Any { self }
    fn to_bytes(&self) -> Result<Vec<u8>, String> {
        rmp_serde::to_vec(self).map_err(|e| e.to_string())
    }
    fn type_name(&self) -> &'static str { std::any::type_name::<T>() }
}

// ---------------------------------------------------------------------------
// Erased value type alias
// ---------------------------------------------------------------------------

/// A heap-allocated, type-erased value that can flow through the graph.
/// Uses `SerializableValue` trait to enable serialization via `to_bytes()`.
pub(crate) type ErasedValue = Arc<dyn SerializableValue>;

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/// Serialize an erased value to msgpack bytes.
pub(crate) fn serialize_erased<T: IncrementalValue>(value: &T) -> Result<Vec<u8>, String> {
    rmp_serde::to_vec(value).map_err(|e| e.to_string())
}

/// Deserialize msgpack bytes into a concrete `T`, returning it as an [`ErasedValue`].
pub(crate) fn deserialize_erased<T: IncrementalValue>(bytes: &[u8]) -> Result<ErasedValue, String> {
    let v: T = rmp_serde::from_slice(bytes).map_err(|e| e.to_string())?;
    Ok(Arc::new(v) as ErasedValue)
}

/// Compute a simple 64-bit hash over msgpack bytes. Used as `ValueHash`.
pub(crate) fn hash_bytes(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Helper to create a deserialize function for a specific type.
pub(crate) fn make_deserialize_fn<T: IncrementalValue>() -> DeserializeFn {
    |bytes| {
        let v: T = rmp_serde::from_slice(bytes).map_err(|e| e.to_string())?;
        Ok(Arc::new(v) as ErasedValue)
    }
}

// ---------------------------------------------------------------------------
// ContextInput / ContextOutput — internal representations
// ---------------------------------------------------------------------------

/// How a value appears on one input slot of a [`TransformContext`].
pub(crate) enum ContextInput {
    Single(ErasedValue),
    Collection {
        elements: Vec<ErasedValue>,
        keys: Vec<u64>,
        /// element keys that are new (added or changed) in this evaluation.
        dirty_keys: Vec<u64>,
        /// element keys that were removed since the last evaluation.
        removed_keys: Vec<u64>,
    },
    /// Required but not yet available.
    Absent,
}

/// How a value appears on one output slot after [`Transform::apply`].
pub(crate) enum ContextOutput {
    Single(ErasedValue),
    Collection(Vec<(u64, ErasedValue)>),
    /// Incremental mutations for collection output.
    CollectionMutations {
        /// Elements to add/set (key, value).
        added: Vec<(u64, ErasedValue)>,
        /// Keys to remove.
        removed: Vec<u64>,
        /// Whether to clear all existing elements first.
        clear: bool,
    },
    /// Transform did not write to this slot.
    Absent,
}

// ---------------------------------------------------------------------------
// TransformContext
// ---------------------------------------------------------------------------

/// Per-invocation typed I/O context injected into [`Transform::apply`].
///
/// Use [`TransformContext::input`] / [`TransformContext::input_collection`] to
/// read inputs, and [`TransformContext::output`] /
/// [`TransformContext::output_collection`] to write results.
pub struct TransformContext {
    pub(crate) inputs: Vec<ContextInput>,
    pub(crate) outputs: Vec<ContextOutput>,
    pub(crate) slot_kinds_in: Vec<SlotKind>,
    pub(crate) slot_kinds_out: Vec<SlotKind>,
}

impl TransformContext {
    pub(crate) fn new(
        inputs: Vec<ContextInput>,
        slot_kinds_in: Vec<SlotKind>,
        slot_kinds_out: Vec<SlotKind>,
    ) -> Self {
        let n_out = slot_kinds_out.len();
        Self {
            inputs,
            outputs: (0..n_out).map(|_| ContextOutput::Absent).collect(),
            slot_kinds_in,
            slot_kinds_out,
        }
    }

    /// Read single-value input slot `slot` as `&T`.
    pub fn input<T: IncrementalValue>(&self, slot: usize) -> Result<&T, TransformError> {
        let kind = self.slot_kinds_in.get(slot)
            .ok_or_else(|| TransformError::new(format!("input slot {slot}: out of range")))?;
        if kind.is_collection {
            return Err(TransformError::new(format!(
                "input slot {slot}: is a collection — use input_collection()"
            )));
        }
        if kind.type_id != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "input slot {slot}: type mismatch (schema: {}, requested: {})",
                kind.type_name, std::any::type_name::<T>()
            )));
        }
        match self.inputs.get(slot) {
            Some(ContextInput::Single(v)) => {
                v.as_any().downcast_ref::<T>()
                    .ok_or_else(|| TransformError::new(format!("input slot {slot}: downcast failed")))
            }
            Some(ContextInput::Absent) | None => {
                Err(TransformError::new(format!("input slot {slot}: no value available")))
            }
            _ => Err(TransformError::new(format!("input slot {slot}: unexpected collection")))
        }
    }

    /// Read collection input slot `slot` as an owned [`CollectionInput<T>`].
    ///
    /// Returns a typed facade that provides key-based access to elements
    /// and exposes incremental diff information (added, changed, removed keys).
    pub fn input_collection<T: IncrementalValue>(
        &self,
        slot: usize,
    ) -> Result<CollectionInput<T>, TransformError> {
        let kind = self.slot_kinds_in.get(slot)
            .ok_or_else(|| TransformError::new(format!("input slot {slot}: out of range")))?;
        if !kind.is_collection {
            return Err(TransformError::new(format!(
                "input slot {slot}: is not a collection — use input()"
            )));
        }
        if kind.type_id != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "input slot {slot}: type mismatch (schema: {}, requested: {})",
                kind.type_name, std::any::type_name::<T>()
            )));
        }
        match self.inputs.get(slot) {
            Some(ContextInput::Collection { elements, keys, dirty_keys, removed_keys }) => {
                // Build a typed HashMap from the erased elements.
                let mut elements_by_key: HashMap<u64, T> = HashMap::with_capacity(elements.len());
                for (i, e) in elements.iter().enumerate() {
                    let typed = e.as_any().downcast_ref::<T>()
                        .expect("collection element type mismatch")
                        .clone();
                    let key = keys.get(i).copied().unwrap_or(0);
                    elements_by_key.insert(key, typed);
                }
                Ok(CollectionInput::new(
                    elements_by_key,
                    keys.clone(),
                    dirty_keys.clone(),
                    vec![], // changed_keys - would need old values to compute
                    removed_keys.clone(),
                ))
            }
            Some(ContextInput::Absent) | None => {
                // Return an empty CollectionInput for absent slots.
                Ok(CollectionInput::new(
                    HashMap::new(),
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                ))
            }
            _ => Err(TransformError::new(format!("input slot {slot}: unexpected single value")))
        }
    }

    /// Write a single value to output slot `slot`.
    pub fn output<T: IncrementalValue>(
        &mut self,
        slot: usize,
        value: T,
    ) -> Result<(), TransformError> {
        let kind = self.slot_kinds_out.get(slot)
            .ok_or_else(|| TransformError::new(format!("output slot {slot}: out of range")))?;
        if kind.is_collection {
            return Err(TransformError::new(format!(
                "output slot {slot}: is collection — use output_collection()"
            )));
        }
        if kind.type_id != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "output slot {slot}: type mismatch (schema: {}, got: {})",
                kind.type_name, std::any::type_name::<T>()
            )));
        }
        self.outputs[slot] = ContextOutput::Single(Arc::new(value));
        Ok(())
    }

    /// Write a collection to output slot `slot`.
    ///
    /// This replaces the entire collection. For incremental updates,
    /// use [`output_collection_builder`](Self::output_collection_builder) instead.
    pub fn output_collection<T: IncrementalValue>(
        &mut self,
        slot: usize,
        items: Vec<T>,
    ) -> Result<(), TransformError> {
        let kind = self.slot_kinds_out.get(slot)
            .ok_or_else(|| TransformError::new(format!("output slot {slot}: out of range")))?;
        if !kind.is_collection {
            return Err(TransformError::new(format!(
                "output slot {slot}: not a collection — use output()"
            )));
        }
        if kind.type_id != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "output slot {slot}: type mismatch (schema: {}, got: {})",
                kind.type_name, std::any::type_name::<T>()
            )));
        }
        let extract_key = kind.extract_key
            .ok_or_else(|| TransformError::new(format!("output slot {slot}: missing KeyExtractor")))?;
        let pairs: Vec<(u64, ErasedValue)> = items.into_iter()
            .map(|item| {
                let erased: ErasedValue = Arc::new(item);
                let key = extract_key(erased.as_ref());
                (key, erased)
            })
            .collect();
        self.outputs[slot] = ContextOutput::Collection(pairs);
        Ok(())
    }

    /// Get a builder for incrementally mutating a collection output slot.
    ///
    /// This allows adding, setting, or removing individual elements without
    /// rebuilding the entire collection. More efficient for large collections
    /// with small deltas.
    pub fn output_collection_builder<T: IncrementalValue>(
        &mut self,
        slot: usize,
    ) -> Result<CollectionOutputBuilder<'_, T>, TransformError> {
        let kind = self.slot_kinds_out.get(slot)
            .ok_or_else(|| TransformError::new(format!("output slot {slot}: out of range")))?;
        if !kind.is_collection {
            return Err(TransformError::new(format!(
                "output slot {slot}: not a collection — use output()"
            )));
        }
        if kind.type_id != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "output slot {slot}: type mismatch (schema: {}, requested: {})",
                kind.type_name, std::any::type_name::<T>()
            )));
        }
        Ok(CollectionOutputBuilder::new(slot, self))
    }

    // -----------------------------------------------------------------------
    // Internal methods for CollectionOutputBuilder
    // -----------------------------------------------------------------------

    /// Add an element to a collection output slot (called by CollectionOutputBuilder).
    pub(crate) fn add_collection_element<T: IncrementalValue>(
        &mut self,
        slot: usize,
        element: T,
    ) -> Result<(), TransformError> {
        let kind = self.slot_kinds_out.get(slot)
            .ok_or_else(|| TransformError::new(format!("output slot {slot}: out of range")))?;
        let extract_key = kind.extract_key
            .ok_or_else(|| TransformError::new(format!("output slot {slot}: missing KeyExtractor")))?;
        let erased: ErasedValue = Arc::new(element);
        let key = extract_key(erased.as_ref());
        
        // Initialize or update the CollectionMutations variant.
        match &mut self.outputs[slot] {
            ContextOutput::Absent => {
                self.outputs[slot] = ContextOutput::CollectionMutations {
                    added: vec![(key, erased)],
                    removed: vec![],
                    clear: false,
                };
            }
            ContextOutput::CollectionMutations { added, .. } => {
                added.push((key, erased));
            }
            _ => {
                return Err(TransformError::new(format!(
                    "output slot {slot}: already written as full collection"
                )));
            }
        }
        Ok(())
    }

    /// Set an element with a specific key in a collection output slot.
    pub(crate) fn set_collection_element<T: IncrementalValue>(
        &mut self,
        slot: usize,
        key: u64,
        element: T,
    ) -> Result<(), TransformError> {
        let kind = self.slot_kinds_out.get(slot)
            .ok_or_else(|| TransformError::new(format!("output slot {slot}: out of range")))?;
        if kind.type_id != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "output slot {slot}: type mismatch"
            )));
        }
        let erased: ErasedValue = Arc::new(element);
        
        match &mut self.outputs[slot] {
            ContextOutput::Absent => {
                self.outputs[slot] = ContextOutput::CollectionMutations {
                    added: vec![(key, erased)],
                    removed: vec![],
                    clear: false,
                };
            }
            ContextOutput::CollectionMutations { added, .. } => {
                // Remove from removed if present, then add.
                added.push((key, erased));
            }
            _ => {
                return Err(TransformError::new(format!(
                    "output slot {slot}: already written as full collection"
                )));
            }
        }
        Ok(())
    }

    /// Remove an element by key from a collection output slot.
    pub(crate) fn remove_collection_element(
        &mut self,
        slot: usize,
        key: u64,
    ) -> Result<(), TransformError> {
        match &mut self.outputs[slot] {
            ContextOutput::Absent => {
                self.outputs[slot] = ContextOutput::CollectionMutations {
                    added: vec![],
                    removed: vec![key],
                    clear: false,
                };
            }
            ContextOutput::CollectionMutations { removed, .. } => {
                removed.push(key);
            }
            _ => {
                return Err(TransformError::new(format!(
                    "output slot {slot}: already written as full collection"
                )));
            }
        }
        Ok(())
    }

    /// Clear all elements from a collection output slot.
    pub(crate) fn clear_collection(&mut self, slot: usize) -> Result<(), TransformError> {
        match &mut self.outputs[slot] {
            ContextOutput::Absent => {
                self.outputs[slot] = ContextOutput::CollectionMutations {
                    added: vec![],
                    removed: vec![],
                    clear: true,
                };
            }
            ContextOutput::CollectionMutations { clear, added, removed } => {
                *clear = true;
                added.clear();
                removed.clear();
            }
            _ => {
                return Err(TransformError::new(format!(
                    "output slot {slot}: already written as full collection"
                )));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ErasedTransform — internal dyn-safe wrapper
// ---------------------------------------------------------------------------

/// Object-safe wrapper over [`Transform`]. `pub(crate)` only.
pub(crate) trait ErasedTransform: Send + Sync {
    fn slot_inputs(&self) -> &[SlotKind];
    fn slot_outputs(&self) -> &[SlotKind];
    fn apply_erased<'a>(
        &'a self,
        ctx: &'a mut TransformContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), TransformError>> + Send + 'a>>;
}

/// Concrete wrapper produced once per `Transform` type during the build phase.
pub(crate) struct TypedErasedTransform<T: Transform> {
    instance: Arc<T>,
    inputs: Vec<SlotKind>,
    outputs: Vec<SlotKind>,
}

impl<T: Transform> TypedErasedTransform<T> {
    pub(crate) fn new(instance: T, inputs: Vec<SlotKind>, outputs: Vec<SlotKind>) -> Self {
        Self { instance: Arc::new(instance), inputs, outputs }
    }
}

impl<T: Transform> ErasedTransform for TypedErasedTransform<T> {
    fn slot_inputs(&self) -> &[SlotKind] { &self.inputs }
    fn slot_outputs(&self) -> &[SlotKind] { &self.outputs }
    fn apply_erased<'a>(
        &'a self,
        ctx: &'a mut TransformContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), TransformError>> + Send + 'a>> {
        Box::pin(self.instance.apply(ctx))
    }
}

// ---------------------------------------------------------------------------
// Transform trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Transform: Send + Sync + 'static {
    /// Declare this transform's slot layout. Called once at build time.
    fn register(ctx: &mut impl TransformRegisterContext) where Self: Sized;

    /// Execute one invocation. Use `?` to propagate [`TransformError`].
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // SlotRegistrar tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_slot_registrar_inputs_outputs() {
        let mut reg = SlotRegistrar::new();
        reg.input::<u64>();
        reg.input_collection::<u64, TestExtractor>();
        reg.output::<String>();
        reg.output_collection::<u64, TestExtractor>();

        assert_eq!(reg.inputs.len(), 2);
        assert_eq!(reg.outputs.len(), 2);
        assert!(!reg.inputs[0].is_collection);
        assert!(reg.inputs[1].is_collection);
        assert!(!reg.outputs[0].is_collection);
        assert!(reg.outputs[1].is_collection);
        assert_eq!(reg.inputs[0].type_id, TypeId::of::<u64>());
        assert_eq!(reg.inputs[1].type_id, TypeId::of::<u64>());
        assert_eq!(reg.outputs[0].type_id, TypeId::of::<String>());
        assert_eq!(reg.outputs[1].type_id, TypeId::of::<u64>());
    }

    struct TestExtractor;
    impl KeyExtractor<u64> for TestExtractor {
        fn extract_key(item: &u64) -> u64 { *item }
    }

    // -----------------------------------------------------------------------
    // TransformContext tests
    // -----------------------------------------------------------------------

    fn make_single_context(input_val: u64) -> TransformContext {
        let erased: ErasedValue = Arc::new(input_val);
        TransformContext::new(
            vec![ContextInput::Single(erased)],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: false,
                extract_key: None,
                deserialize: |_| Err("not needed".to_string()),
            }],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: false,
                extract_key: None,
                deserialize: |_| Err("not needed".to_string()),
            }],
        )
    }

    fn make_collection_context(
        elements: Vec<(u64, u64)>,
    ) -> TransformContext {
        let keys: Vec<u64> = elements.iter().map(|(k, _)| *k).collect();
        let erased: Vec<ErasedValue> = elements.iter()
            .map(|(_, v)| Arc::new(*v) as ErasedValue).collect();
        TransformContext::new(
            vec![ContextInput::Collection {
                elements: erased,
                keys: keys.clone(),
                dirty_keys: keys.clone(),
                removed_keys: vec![],
            }],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: true,
                extract_key: Some(|a: &dyn Any| {
                    *a.downcast_ref::<u64>().unwrap()
                }),
                deserialize: |_| Err("not needed".to_string()),
            }],
            vec![],
        )
    }

    #[test]
    fn test_context_single_input() {
        let ctx = make_single_context(42);
        assert_eq!(*ctx.input::<u64>(0).unwrap(), 42);
    }

    #[test]
    fn test_context_input_type_mismatch_error() {
        let ctx = make_single_context(42);
        let err = ctx.input::<String>(0).unwrap_err();
        assert!(err.message.contains("type mismatch"));
    }

    #[test]
    fn test_context_input_out_of_range() {
        let ctx = make_single_context(42);
        let err = ctx.input::<u64>(5).unwrap_err();
        assert!(err.message.contains("out of range"));
    }

    #[test]
    fn test_context_input_collection_as_single_error() {
        let ctx = make_collection_context(vec![(1, 10)]);
        let err = ctx.input::<u64>(0).unwrap_err();
        assert!(err.message.contains("collection"));
    }

    #[test]
    fn test_context_output() {
        let mut ctx = TransformContext::new(
            vec![],
            vec![],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: false,
                extract_key: None,
                deserialize: |_| Err("not needed".to_string()),
            }],
        );
        ctx.output(0, 42u64).unwrap();
        match &ctx.outputs[0] {
            ContextOutput::Single(v) => {
                let val = v.as_any().downcast_ref::<u64>().unwrap();
                assert_eq!(*val, 42);
            }
            _ => panic!("expected Single"),
        }
    }

    #[test]
    fn test_context_output_collection() {
        let mut ctx = TransformContext::new(
            vec![],
            vec![],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: true,
                extract_key: Some(|a: &dyn Any| *a.downcast_ref::<u64>().unwrap()),
                deserialize: |_| Err("not needed".to_string()),
            }],
        );
        ctx.output_collection(0, vec![3u64, 1u64, 2u64]).unwrap();
        match &ctx.outputs[0] {
            ContextOutput::Collection(pairs) => {
                assert_eq!(pairs.len(), 3);
                // Keys from ByValue: 3, 1, 2
                let mut keys: Vec<u64> = pairs.iter().map(|(k, _)| *k).collect();
                keys.sort_unstable();
                assert_eq!(keys, vec![1, 2, 3]);
            }
            _ => panic!("expected Collection"),
        }
    }

    #[test]
    fn test_context_output_type_mismatch() {
        let mut ctx = TransformContext::new(
            vec![],
            vec![],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: false,
                extract_key: None,
                deserialize: |_| Err("not needed".to_string()),
            }],
        );
        let err = ctx.output::<String>(0, "hello".to_string()).unwrap_err();
        assert!(err.message.contains("type mismatch"));
    }

    // -----------------------------------------------------------------------
    // CollectionInput tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_collection_input_basic() {
        let mut elements = std::collections::HashMap::new();
        elements.insert(1u64, 10u64);
        elements.insert(2u64, 20u64);
        let ci = CollectionInput::new(
            elements,
            vec![1, 2],
            vec![1],
            vec![],
            vec![],
        );
        assert_eq!(ci.len(), 2);
        assert!(ci.contains(1));
        assert!(!ci.contains(3));
        assert_eq!(*ci.get(1).unwrap(), 10);
        assert!(ci.has_changes());
    }

    #[test]
    fn test_collection_input_empty() {
        let ci: CollectionInput<u64> = CollectionInput::new(
            std::collections::HashMap::new(),
            vec![],
            vec![],
            vec![],
            vec![],
        );
        assert!(ci.is_empty());
        assert_eq!(ci.len(), 0);
        assert!(!ci.has_changes());
    }

    #[test]
    fn test_collection_input_diff_info() {
        let mut elements = std::collections::HashMap::new();
        elements.insert(1u64, 10u64);
        elements.insert(3u64, 30u64);
        let ci = CollectionInput::new(
            elements,
            vec![1, 3],
            vec![1],       // added
            vec![],        // changed
            vec![2],       // removed
        );
        assert_eq!(ci.added_keys(), &[1]);
        assert_eq!(ci.removed_keys(), &[2]);
        assert!(ci.changed_keys().is_empty());
    }

    // -----------------------------------------------------------------------
    // CollectionOutputBuilder tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_collection_builder_add() {
        let mut ctx = TransformContext::new(
            vec![],
            vec![],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: true,
                extract_key: Some(|a: &dyn Any| *a.downcast_ref::<u64>().unwrap()),
                deserialize: |_| Err("not needed".to_string()),
            }],
        );
        {
            let mut builder = ctx.output_collection_builder::<u64>(0).unwrap();
            builder.add(10u64).unwrap();
            builder.add(20u64).unwrap();
        }
        match &ctx.outputs[0] {
            ContextOutput::CollectionMutations { added, .. } => {
                assert_eq!(added.len(), 2);
                assert_eq!(added[0].0, 10);
                assert_eq!(added[1].0, 20);
            }
            _ => panic!("expected CollectionMutations"),
        }
    }

    #[test]
    fn test_collection_builder_remove() {
        let mut ctx = TransformContext::new(
            vec![],
            vec![],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: true,
                extract_key: Some(|a: &dyn Any| *a.downcast_ref::<u64>().unwrap()),
                deserialize: |_| Err("not needed".to_string()),
            }],
        );
        {
            let mut builder = ctx.output_collection_builder::<u64>(0).unwrap();
            builder.remove(42).unwrap();
        }
        match &ctx.outputs[0] {
            ContextOutput::CollectionMutations { removed, .. } => {
                assert_eq!(removed, &[42]);
            }
            _ => panic!("expected CollectionMutations"),
        }
    }

    #[test]
    fn test_collection_builder_clear() {
        let mut ctx = TransformContext::new(
            vec![],
            vec![],
            vec![SlotKind {
                type_id: TypeId::of::<u64>(),
                type_name: "u64",
                is_collection: true,
                extract_key: Some(|a: &dyn Any| *a.downcast_ref::<u64>().unwrap()),
                deserialize: |_| Err("not needed".to_string()),
            }],
        );
        {
            let mut builder = ctx.output_collection_builder::<u64>(0).unwrap();
            builder.clear().unwrap();
            builder.add(99u64).unwrap();
        }
        match &ctx.outputs[0] {
            ContextOutput::CollectionMutations { added, clear, .. } => {
                assert!(*clear);
                assert_eq!(added.len(), 1);
                assert_eq!(added[0].0, 99);
            }
            _ => panic!("expected CollectionMutations"),
        }
    }

    // -----------------------------------------------------------------------
    // TransformError tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_transform_error_display() {
        let err = TransformError::new("something broke");
        assert_eq!(err.to_string(), "something broke");

        let err2 = TransformError::with_source("outer", "inner");
        assert_eq!(err2.to_string(), "outer: inner");
    }

    // -----------------------------------------------------------------------
    // IncrementalValue blanket
    // -----------------------------------------------------------------------

    #[test]
    fn test_u64_is_incremental_value() {
        fn check<T: IncrementalValue>() {}
        check::<u64>();
        check::<String>();
        check::<i32>();
    }

    // -----------------------------------------------------------------------
    // hash_bytes determinism
    // -----------------------------------------------------------------------

    #[test]
    fn test_hash_bytes_deterministic() {
        let bytes = b"hello world";
        assert_eq!(hash_bytes(bytes), hash_bytes(bytes));
        assert_ne!(hash_bytes(b"hello"), hash_bytes(b"world"));
    }
}
