//! Public `Transform` trait + `TransformContext` + supporting types.
//!
//! ## Design: One Public Trait, Zero Internal Leakage
//!
//! External crates only see:
//! - `Transform` – the trait to implement
//! - `TransformContext` – typed I/O injected per `apply()` call
//! - `TransformSchema` – opaque slot-layout builder (returned by `schema()`)
//! - `TransformError`, `IncrementalValue`, `KeyExtractor`,
//!   `CollectionInput`, `CollectionChange`
//!
//! All internal execution machinery (`SlotInput`, `SlotOutput`, `Value`, etc.)
//! stays `pub(crate)` in this module.

use std::any::{Any, TypeId};
use std::future::Future;
use std::pin::Pin;
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
/// Implement on a zero-sized marker struct.  Key must be:
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
// SlotSpec (pub(crate))
// ---------------------------------------------------------------------------

/// Erased type info for one slot.
#[derive(Clone)]
pub(crate) struct SlotSpec {
    pub(crate) type_id:    TypeId,
    pub(crate) type_name:  &'static str,
    pub(crate) is_col:     bool,
    /// Downcast a Value → Box<T> (erased).
    pub(crate) downcast:   fn(&crate::value::Value, &crate::value::ValueTypeRegistry)
                              -> Result<Box<dyn Any + Send + Sync>, String>,
    /// Extract stable key from Value (collection slots only).
    pub(crate) extract_key: Option<fn(&crate::value::Value, &crate::value::ValueTypeRegistry) -> u64>,
    /// Build an ErasedCollection from graph elements (collection input slots only).
    /// Signature: (all_values: Vec<Value>, dirty_keys: Vec<u64>, registry) → ErasedCollection
    pub(crate) build_collection: Option<
        fn(Vec<crate::value::Value>, Vec<u64>, &crate::value::ValueTypeRegistry) -> ErasedCollection
    >,
    /// Register this slot's type into the registry builder (build phase only).
    pub(crate) register: fn(&mut crate::value::ValueTypeRegistryBuilder),
}

impl std::fmt::Debug for SlotSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SlotSpec({}, col={})", self.type_name, self.is_col)
    }
}

fn slot_register<T: IncrementalValue>(b: &mut crate::value::ValueTypeRegistryBuilder) {
    let _ = b.register::<T>();
}

fn slot_downcast<T: IncrementalValue>(
    v: &crate::value::Value,
    reg: &crate::value::ValueTypeRegistry,
) -> Result<Box<dyn Any + Send + Sync>, String> {
    reg.downcast_value::<T>(v)
        .map(|t| Box::new(t) as Box<dyn Any + Send + Sync>)
        .map_err(|e| e.message)
}

fn slot_extract_key<T: IncrementalValue, K: KeyExtractor<T>>(
    v: &crate::value::Value,
    reg: &crate::value::ValueTypeRegistry,
) -> u64 {
    let t: T = reg.downcast_value::<T>(v).expect("KeyExtractor: type mismatch");
    K::extract_key(&t)
}

fn slot_build_collection<T: IncrementalValue>(
    values:     Vec<crate::value::Value>,
    dirty_keys: Vec<u64>,
    reg:        &crate::value::ValueTypeRegistry,
) -> ErasedCollection {
    let dirty_set: std::collections::HashSet<u64> = dirty_keys.into_iter().collect();
    let mut elements: Vec<T> = vec![];
    let mut added:    Vec<T> = vec![];

    for v in &values {
        if let Ok(t) = reg.downcast_value::<T>(v) {
            let h = crate::value::hash_value(v, reg);
            if dirty_set.contains(&h) {
                added.push(t.clone());
            }
            elements.push(t);
        }
    }

    let diff = CollectionChange { added, changed: vec![], removed: vec![] };
    ErasedCollection::new(elements, diff)
}

impl SlotSpec {
    fn single<T: IncrementalValue>() -> Self {
        Self {
            type_id:          TypeId::of::<T>(),
            type_name:        std::any::type_name::<T>(),
            is_col:           false,
            register:         slot_register::<T>,
            downcast:         slot_downcast::<T>,
            extract_key:      None,
            build_collection: None,
        }
    }

    fn collection<T: IncrementalValue, K: KeyExtractor<T>>() -> Self {
        Self {
            type_id:          TypeId::of::<T>(),
            type_name:        std::any::type_name::<T>(),
            is_col:           true,
            register:         slot_register::<T>,
            downcast:         slot_downcast::<T>,
            extract_key:      Some(slot_extract_key::<T, K>),
            build_collection: Some(slot_build_collection::<T>),
        }
    }
}

// ---------------------------------------------------------------------------
// TransformSchema (public opaque)
// ---------------------------------------------------------------------------

/// Describes the typed slot layout of a [`Transform`].
///
/// Constructed via chaining: `TransformSchema::new().input::<A>().output::<B>()`.
/// Fields are private; the engine reads them through `pub(crate)` accessors.
#[derive(Clone, Debug)]
pub struct TransformSchema {
    pub(crate) inputs:  Vec<SlotSpec>,
    pub(crate) outputs: Vec<SlotSpec>,
}

impl TransformSchema {
    pub fn new() -> Self { Self { inputs: vec![], outputs: vec![] } }

    /// Append a single-value input slot of type `T`.
    pub fn input<T: IncrementalValue>(mut self) -> Self {
        self.inputs.push(SlotSpec::single::<T>());
        self
    }

    /// Append a collection input slot (gather) of type `T`, keyed by `K`.
    pub fn input_collection<T: IncrementalValue, K: KeyExtractor<T>>(mut self) -> Self {
        self.inputs.push(SlotSpec::collection::<T, K>());
        self
    }

    /// Append a single-value output slot of type `T`.
    pub fn output<T: IncrementalValue>(mut self) -> Self {
        self.outputs.push(SlotSpec::single::<T>());
        self
    }

    /// Append a collection output slot (spread) of type `T`, keyed by `K`.
    pub fn output_collection<T: IncrementalValue, K: KeyExtractor<T>>(mut self) -> Self {
        self.outputs.push(SlotSpec::collection::<T, K>());
        self
    }

    /// Register all slot types into the registry builder (build phase only).
    pub(crate) fn register_all_into(&self, b: &mut crate::value::ValueTypeRegistryBuilder) {
        for s in self.inputs.iter().chain(self.outputs.iter()) {
            (s.register)(b);
        }
    }
}

impl Default for TransformSchema { fn default() -> Self { Self::new() } }

// ---------------------------------------------------------------------------
// CollectionChange / CollectionInput (public)
// ---------------------------------------------------------------------------

/// Incremental diff for a collection input slot.
#[derive(Debug, Clone)]
pub struct CollectionChange<T> {
    pub added:   Vec<T>,
    pub changed: Vec<(T, T)>,
    pub removed: Vec<u64>,
}

impl<T> Default for CollectionChange<T> {
    fn default() -> Self { Self { added: vec![], changed: vec![], removed: vec![] } }
}

/// Typed view of a gathered collection input.
pub struct CollectionInput<'a, T> {
    pub elements: &'a [T],
    pub diff:     &'a CollectionChange<T>,
}

pub(crate) struct TypedCollection<T: 'static> {
    pub elements: Vec<T>,
    pub diff:     CollectionChange<T>,
}

pub(crate) struct ErasedCollection {
    inner:   Box<dyn Any + Send + Sync>,
    type_id: TypeId,
}

impl ErasedCollection {
    pub(crate) fn new<T: IncrementalValue>(elements: Vec<T>, diff: CollectionChange<T>) -> Self {
        let type_id = TypeId::of::<T>();
        Self { inner: Box::new(TypedCollection { elements, diff }), type_id }
    }

    pub(crate) fn type_id(&self) -> TypeId { self.type_id }

    pub(crate) fn get<T: IncrementalValue>(&self) -> Option<&TypedCollection<T>> {
        self.inner.downcast_ref::<TypedCollection<T>>()
    }
}

// ---------------------------------------------------------------------------
// ContextOutput (pub(crate))
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(crate) enum ContextOutput {
    Single(crate::value::Value, crate::value::ValueHash),
    Collection(Vec<(u64, crate::value::Value, crate::value::ValueHash)>),
}

// ---------------------------------------------------------------------------
// TransformContext (public)
// ---------------------------------------------------------------------------

/// Per-invocation typed I/O context injected into [`Transform::apply`].
pub struct TransformContext {
    /// Single-value input slots; index = slot number.
    pub(crate) single_inputs:      Vec<Option<Box<dyn Any + Send + Sync>>>,
    /// Collection input slots; index = slot number.
    pub(crate) collection_inputs:  Vec<Option<ErasedCollection>>,
    /// True iff slot[i] is a collection.
    pub(crate) slot_is_collection: Vec<bool>,
    /// Output slots, initially None, filled by transform.
    pub(crate) outputs:            Vec<Option<ContextOutput>>,
    pub(crate) schema:             Arc<TransformSchema>,
    pub(crate) registry:           Arc<crate::value::ValueTypeRegistry>,
}

impl TransformContext {
    pub(crate) fn new(schema: Arc<TransformSchema>, registry: Arc<crate::value::ValueTypeRegistry>) -> Self {
        let n_in  = schema.inputs.len();
        let n_out = schema.outputs.len();
        let slot_is_collection = schema.inputs.iter().map(|s| s.is_col).collect();
        Self {
            single_inputs:      (0..n_in).map(|_| None).collect(),
            collection_inputs:  (0..n_in).map(|_| None).collect(),
            slot_is_collection,
            outputs:            (0..n_out).map(|_| None).collect(),
            schema,
            registry,
        }
    }

    /// Read single-value input slot `slot` as `&T`.
    pub fn input<T: IncrementalValue>(&self, slot: usize) -> Result<&T, TransformError> {
        let is_col = self.slot_is_collection.get(slot).copied().unwrap_or(false);
        if is_col {
            return Err(TransformError::new(format!(
                "slot {slot}: is a collection — use input_collection()"
            )));
        }
        self.single_inputs
            .get(slot)
            .and_then(|o| o.as_ref())
            .and_then(|b| b.downcast_ref::<T>())
            .ok_or_else(|| TransformError::new(format!(
                "slot {slot}: no value or type mismatch (expected {})",
                std::any::type_name::<T>()
            )))
    }

    /// Read collection input slot `slot` as `CollectionInput<T>`.
    pub fn input_collection<T: IncrementalValue>(
        &self,
        slot: usize,
    ) -> Result<CollectionInput<'_, T>, TransformError> {
        let is_col = self.slot_is_collection.get(slot).copied().unwrap_or(false);
        if !is_col {
            return Err(TransformError::new(format!(
                "slot {slot}: is not a collection — use input()"
            )));
        }
        let erased = self.collection_inputs
            .get(slot)
            .and_then(|o| o.as_ref())
            .ok_or_else(|| TransformError::new(format!("slot {slot}: no collection data")))?;

        if erased.type_id() != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "slot {slot}: type mismatch in collection (expected {})",
                std::any::type_name::<T>()
            )));
        }
        let typed = erased.get::<T>()
            .ok_or_else(|| TransformError::new(format!("slot {slot}: internal downcast failed")))?;

        Ok(CollectionInput {
            elements: &typed.elements,
            diff:     &typed.diff,
        })
    }

    /// Write single value to output slot `slot`.
    pub fn output<T: IncrementalValue>(
        &mut self,
        slot: usize,
        value: T,
    ) -> Result<(), TransformError> {
        if slot >= self.outputs.len() {
            return Err(TransformError::new(format!("output slot {slot}: out of range")));
        }
        let spec = &self.schema.outputs[slot];
        if spec.is_col {
            return Err(TransformError::new(format!(
                "output slot {slot}: is collection — use output_collection()"
            )));
        }
        if spec.type_id != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "output slot {slot}: type mismatch (schema: {}, got: {})",
                spec.type_name, std::any::type_name::<T>()
            )));
        }
        let v = self.registry.make_value(value)
            .map_err(|e| TransformError::new(e.message))?;
        let h = crate::value::hash_value(&v, &self.registry);
        self.outputs[slot] = Some(ContextOutput::Single(v, h));
        Ok(())
    }

    /// Write collection values to output slot `slot`.
    pub fn output_collection<T: IncrementalValue>(
        &mut self,
        slot: usize,
        items: Vec<T>,
    ) -> Result<(), TransformError> {
        if slot >= self.outputs.len() {
            return Err(TransformError::new(format!("output slot {slot}: out of range")));
        }
        let spec = &self.schema.outputs[slot];
        if !spec.is_col {
            return Err(TransformError::new(format!(
                "output slot {slot}: not collection — use output()"
            )));
        }
        if spec.type_id != TypeId::of::<T>() {
            return Err(TransformError::new(format!(
                "output slot {slot}: type mismatch (schema: {}, got: {})",
                spec.type_name, std::any::type_name::<T>()
            )));
        }
        let extract_key = spec.extract_key
            .ok_or_else(|| TransformError::new("output slot: missing key extractor"))?;
        let mut pairs = Vec::with_capacity(items.len());
        for item in items {
            let v = self.registry.make_value(item)
                .map_err(|e| TransformError::new(e.message))?;
            let h = crate::value::hash_value(&v, &self.registry);
            let key = extract_key(&v, &self.registry);
            pairs.push((key, v, h));
        }
        self.outputs[slot] = Some(ContextOutput::Collection(pairs));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Transform trait (public)
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Transform: Send + Sync + 'static {
    /// Declare the slot layout.  Called as `MyTransform::schema()`.
    fn schema() -> TransformSchema where Self: Sized;

    /// Execute one invocation.  Use `?` to propagate `TransformError`.
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError>;
}

// ---------------------------------------------------------------------------
// ErasedTransform (pub(crate)) — object-safe wrapper
// ---------------------------------------------------------------------------

/// SAFETY wrapper to move a raw pointer across the Send boundary.
/// The pointer is stored as a `usize` to avoid rustc's raw-pointer Send check.
/// Users MUST ensure the pointed-to value outlives any future that uses it.
struct SendablePtr(usize);
unsafe impl Send for SendablePtr {}

impl SendablePtr {
    fn from_mut<T>(p: *mut T) -> Self { Self(p as usize) }
    unsafe fn recover<T>(&self) -> *mut T { self.0 as *mut T }
}

type ApplyFn = Arc<
    dyn for<'a> Fn(&'a mut TransformContext)
        -> Pin<Box<dyn Future<Output = Result<(), TransformError>> + Send + 'a>>
    + Send + Sync,
>;

/// Object-safe wrapper holding a boxed `Transform` instance + its schema.
#[derive(Clone)]
pub(crate) struct ErasedTransform {
    pub(crate) schema:    Arc<TransformSchema>,
    pub(crate) apply_fn:  ApplyFn,
}

impl ErasedTransform {
    pub(crate) fn new<T: Transform>(schema: TransformSchema, instance: T) -> Self {
        let instance = Arc::new(instance);
        let apply_fn: ApplyFn = Arc::new(move |ctx: &mut TransformContext| {
            let inst = Arc::clone(&instance);
            let ptr = SendablePtr::from_mut(ctx as *mut TransformContext);
            Box::pin(async move {
                // SAFETY: ctx is alive for the full await duration.
                let ctx_ref = unsafe { &mut *ptr.recover::<TransformContext>() };
                inst.apply(ctx_ref).await
            })
        });
        Self { schema: Arc::new(schema), apply_fn }
    }

    pub(crate) async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        (self.apply_fn)(ctx).await
    }
}

impl std::fmt::Debug for ErasedTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ErasedTransform({}→{})", self.schema.inputs.len(), self.schema.outputs.len())
    }
}