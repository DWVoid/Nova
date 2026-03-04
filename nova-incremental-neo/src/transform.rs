//! Public [`Transform`] trait, [`TransformContext`], and supporting types.
//!
//! ## What external crates see
//! - [`Transform`] — the trait to implement.
//! - [`TransformContext`] — typed I/O passed to [`Transform::apply`].
//! - [`TransformRegisterContext`] — slot-declaration DSL (sealed, not externally implementable).
//! - [`TransformError`], [`IncrementalValue`], [`KeyExtractor`],
//!   [`CollectionInput`], [`CollectionChange`].
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
// CollectionChange / CollectionInput
// ---------------------------------------------------------------------------

/// Incremental diff for a collection input slot.
#[derive(Debug, Clone)]
pub struct CollectionChange<T> {
    pub added: Vec<T>,
    pub changed: Vec<(T, T)>,
    pub removed: Vec<u64>,
}

impl<T> Default for CollectionChange<T> {
    fn default() -> Self {
        Self { added: vec![], changed: vec![], removed: vec![] }
    }
}

/// Typed view of a gathered collection input slot.
pub struct CollectionInput<'a, T> {
    pub elements: &'a [T],
    pub diff: &'a CollectionChange<T>,
}

// ---------------------------------------------------------------------------
// SlotKind — internal slot descriptor
// ---------------------------------------------------------------------------

/// Minimal type descriptor for one slot. `pub(crate)` only.
#[derive(Clone, Debug)]
pub(crate) struct SlotKind {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) is_collection: bool,
    /// For collection output slots: extracts a stable `u64` key from a boxed value.
    pub(crate) extract_key: Option<fn(&dyn Any) -> u64>,
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
        });
    }
    fn input_collection<T: IncrementalValue, K: KeyExtractor<T>>(&mut self) {
        self.inputs.push(SlotKind {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            is_collection: true,
            extract_key: None,
        });
    }
    fn output<T: IncrementalValue>(&mut self) {
        self.outputs.push(SlotKind {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            is_collection: false,
            extract_key: None,
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
        });
    }
}

// ---------------------------------------------------------------------------
// Erased value helpers
// ---------------------------------------------------------------------------

/// A heap-allocated, type-erased value that can flow through the graph.
pub(crate) type ErasedValue = Arc<dyn Any + Send + Sync>;

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
                v.downcast_ref::<T>()
                    .ok_or_else(|| TransformError::new(format!("input slot {slot}: downcast failed")))
            }
            Some(ContextInput::Absent) | None => {
                Err(TransformError::new(format!("input slot {slot}: no value available")))
            }
            _ => Err(TransformError::new(format!("input slot {slot}: unexpected collection")))
        }
    }

    /// Read collection input slot `slot` as [`CollectionInput<T>`].
    pub fn input_collection<T: IncrementalValue>(
        &self,
        slot: usize,
    ) -> Result<CollectionInput<'_, T>, TransformError> {
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
            Some(ContextInput::Collection { elements, keys: _, dirty_keys, removed_keys }) => {
                // Downcast all elements into a typed slice.
                // We use a thread-local scratch buffer to avoid allocations in the common
                // path where the transform just reads; but for simplicity in this iteration
                // we allocate.
                let typed_elements: Vec<&T> = elements.iter()
                    .map(|e| e.downcast_ref::<T>()
                        .expect("collection element type mismatch"))
                    .collect();
                // We can't return &[T] from &Vec<&T> directly, so we store a Vec<T> scratch.
                // This is a known limitation; in a future iteration we can pre-box as &[T].
                // For now return via a helper that stores in the context.
                // DESIGN NOTE: CollectionInput borrows from self, so we need typed storage.
                // We store the typed elements in a Box leaked into the ContextInput — this
                // is safe for the lifetime of this context. However, for simplicity we
                // return an error and note this requires refactoring.
                //
                // Actually, let us just re-downcast inline. The lifetime is 'self so we
                // can return references into the ErasedValue Arcs which are held in self.inputs.
                let _ = typed_elements; // drop the Vec<&T>

                // Build diff from dirty/removed keys.
                // We don't have the old values here for "changed" — the runner will have
                // pre-split into added vs changed. For this iteration, dirty_keys = added.
                let added: Vec<T> = elements.iter()
                    .zip(/* keys info not directly accessible here */ std::iter::repeat(0u64))
                    .filter_map(|(e, _)| e.downcast_ref::<T>().cloned())
                    .collect::<Vec<T>>();
                // For now expose the simple form: elements slice rebuilt, diff from added/removed.
                // A proper implementation would store typed scratch in the context.
                // This is tracked as CHANGES.md item.
                let _ = added;

                // Return a simplified view using zero-copy downcast refs.
                // Limitation: we can't return &[T] from &[Arc<dyn Any>] without a scratch buffer.
                // For the initial implementation, we store the typed scratch in a Box<[T]>
                // that the context owns. We need to restructure TransformContext for this.
                // CHANGES: input_collection needs a pre-typed scratch buffer — see CHANGES.md.
                Err(TransformError::new("input_collection: not yet fully implemented — see CHANGES.md"))
            }
            Some(ContextInput::Absent) | None => {
                Err(TransformError::new(format!("input slot {slot}: no collection available")))
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
