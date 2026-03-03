//! Public `Transform` trait + `TransformContext` + supporting types.
//!
//! ## Design: One Public Trait, Zero Internal Leakage
//!
//! External crates only see:
//! - `Transform` – the trait to implement
//! - `TransformContext` – typed I/O injected per `apply()` call
//! - `TransformRegisterContext` – slot declaration DSL (sealed, not implementable externally)
//! - `TransformError`, `IncrementalValue`, `KeyExtractor`,
//!   `CollectionInput`, `CollectionChange`
//!
//! All internal execution machinery (`SlotSpec`, `SlotLayout`, `Value`, etc.)
//! stays `pub(crate)` in this module.

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
// ---------------------------------------------------------------------------
// SlotInfo (pub(crate)) — type identity for one slot
// ---------------------------------------------------------------------------

/// Minimal erased type info for one slot: identity + collection flag only.
/// Stored in [`SlotLayout`] and used at runtime for type checking.
#[derive(Clone, Debug)]
pub(crate) struct SlotInfo {
    pub(crate) type_id:   TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) is_col:    bool,
}

// ---------------------------------------------------------------------------
// SlotSpec (pub(crate), build-time only) — SlotInfo + dispatch fn ptrs
// ---------------------------------------------------------------------------

/// Full slot descriptor used only during [`crate::engine::EngineBuilder::build()`].
/// The dispatch fn pointers are extracted into a [`DispatchTable`]; then only
/// [`SlotInfo`] survives in [`SlotLayout`].
pub(crate) struct SlotSpec {
    pub(crate) info:             SlotInfo,
    /// Register T into the registry builder.
    pub(crate) register:         fn(&mut crate::value::ValueTypeRegistryBuilder),
    /// Downcast a Value → Box<dyn Any> (for feeding into TransformContext).
    pub(crate) downcast:         fn(&crate::value::Value, &crate::value::ValueTypeRegistry)
                                    -> Result<Box<dyn Any + Send + Sync>, String>,
    /// Extract stable key from Value (collection output slots only).
    pub(crate) extract_key:      Option<fn(&crate::value::Value, &crate::value::ValueTypeRegistry) -> u64>,
    /// Reconstruct a typed ErasedCollection from raw graph elements (collection input slots only).
    pub(crate) build_collection: Option<
        fn(Vec<crate::value::Value>, Vec<u64>, &crate::value::ValueTypeRegistry) -> ErasedCollection
    >,
}

// ---------------------------------------------------------------------------
// DispatchEntry + DispatchTable (pub(crate), runtime)
// ---------------------------------------------------------------------------

/// Erased dispatch functions for one value type.
/// Built from [`SlotSpec`] fn pointers during the build phase; stored in
/// [`DispatchTable`] keyed by `TypeId`.
#[derive(Clone)]
pub(crate) struct DispatchEntry {
    pub(crate) downcast:         fn(&crate::value::Value, &crate::value::ValueTypeRegistry)
                                    -> Result<Box<dyn Any + Send + Sync>, String>,
    pub(crate) extract_key:      Option<fn(&crate::value::Value, &crate::value::ValueTypeRegistry) -> u64>,
    pub(crate) build_collection: Option<
        fn(Vec<crate::value::Value>, Vec<u64>, &crate::value::ValueTypeRegistry) -> ErasedCollection
    >,
}

/// Per-transform dispatch table, indexed by slot index (inputs then outputs).
/// Stored alongside the transform's [`SlotLayout`] in `Topology`.
#[derive(Clone, Default)]
pub(crate) struct DispatchTable {
    pub(crate) inputs:  Vec<DispatchEntry>,
    pub(crate) outputs: Vec<DispatchEntry>,
}

// ---------------------------------------------------------------------------
// SlotSpec constructors + helpers
// ---------------------------------------------------------------------------

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
    pub(crate) fn single<T: IncrementalValue>() -> Self {
        Self {
            info:             SlotInfo {
                type_id:   TypeId::of::<T>(),
                type_name: std::any::type_name::<T>(),
                is_col:    false,
            },
            register:         slot_register::<T>,
            downcast:         slot_downcast::<T>,
            extract_key:      None,
            build_collection: None,
        }
    }

    pub(crate) fn collection<T: IncrementalValue, K: KeyExtractor<T>>() -> Self {
        Self {
            info:             SlotInfo {
                type_id:   TypeId::of::<T>(),
                type_name: std::any::type_name::<T>(),
                is_col:    true,
            },
            register:         slot_register::<T>,
            downcast:         slot_downcast::<T>,
            extract_key:      Some(slot_extract_key::<T, K>),
            build_collection: Some(slot_build_collection::<T>),
        }
    }

    /// Extract the [`DispatchEntry`] from this spec (used during build phase).
    pub(crate) fn dispatch_entry(&self) -> DispatchEntry {
        DispatchEntry {
            downcast:         self.downcast,
            extract_key:      self.extract_key,
            build_collection: self.build_collection,
        }
    }
}

// ---------------------------------------------------------------------------
// SlotLayout (pub(crate)) — internal replacement for the old public TransformSchema
// ---------------------------------------------------------------------------

/// Internal slot description for a transform.  Not part of the public API.
/// Produced by [`TransformRegistrar`] during the build phase and stored in
/// `Topology`.  Contains only type identity — no fn pointers.
#[derive(Clone, Debug)]
pub(crate) struct SlotLayout {
    pub(crate) inputs:  Vec<SlotInfo>,
    pub(crate) outputs: Vec<SlotInfo>,
}

impl SlotLayout {
    pub(crate) fn new(inputs: Vec<SlotInfo>, outputs: Vec<SlotInfo>) -> Self {
        Self { inputs, outputs }
    }
}

// ---------------------------------------------------------------------------
// TransformRegisterContext — sealed public trait
// ---------------------------------------------------------------------------

mod sealed { pub trait Sealed {} }

/// DSL for declaring a transform's input/output slot layout.
///
/// Passed to [`Transform::register`] once at build time.  Users call the
/// fluent methods to declare slots; errors are accumulated and surfaced later
/// by [`EngineBuilder::build`].
///
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

/// Build-phase accumulator implementing [`TransformRegisterContext`].
/// Holds the full [`SlotSpec`] (including fn pointers) during registration;
/// produces [`SlotLayout`] + [`DispatchTable`] via [`finish`](Self::finish).
pub(crate) struct TransformRegistrar {
    inputs:  Vec<SlotSpec>,
    outputs: Vec<SlotSpec>,
}

impl TransformRegistrar {
    pub(crate) fn new() -> Self { Self { inputs: vec![], outputs: vec![] } }

    /// Register all slot types into the builder and produce the frozen
    /// [`SlotLayout`] + [`DispatchTable`] for runtime use.
    pub(crate) fn finish_into(
        self,
        builder: &mut crate::value::ValueTypeRegistryBuilder,
    ) -> (SlotLayout, DispatchTable) {
        let mut layout_inputs  = Vec::with_capacity(self.inputs.len());
        let mut layout_outputs = Vec::with_capacity(self.outputs.len());
        let mut disp_inputs    = Vec::with_capacity(self.inputs.len());
        let mut disp_outputs   = Vec::with_capacity(self.outputs.len());

        for spec in self.inputs {
            (spec.register)(builder);
            disp_inputs.push(spec.dispatch_entry());
            layout_inputs.push(spec.info);
        }
        for spec in self.outputs {
            (spec.register)(builder);
            disp_outputs.push(spec.dispatch_entry());
            layout_outputs.push(spec.info);
        }

        (
            SlotLayout::new(layout_inputs, layout_outputs),
            DispatchTable { inputs: disp_inputs, outputs: disp_outputs },
        )
    }

    /// Convenience: produce a [`SlotLayout`] without registering types.
    /// Used in tests that build a `TransformContext` directly.
    pub(crate) fn finish(self) -> SlotLayout {
        SlotLayout::new(
            self.inputs.into_iter().map(|s| s.info).collect(),
            self.outputs.into_iter().map(|s| s.info).collect(),
        )
    }
}

impl sealed::Sealed for TransformRegistrar {}

impl TransformRegisterContext for TransformRegistrar {
    fn input<T: IncrementalValue>(&mut self) {
        self.inputs.push(SlotSpec::single::<T>());
    }
    fn input_collection<T: IncrementalValue, K: KeyExtractor<T>>(&mut self) {
        self.inputs.push(SlotSpec::collection::<T, K>());
    }
    fn output<T: IncrementalValue>(&mut self) {
        self.outputs.push(SlotSpec::single::<T>());
    }
    fn output_collection<T: IncrementalValue, K: KeyExtractor<T>>(&mut self) {
        self.outputs.push(SlotSpec::collection::<T, K>());
    }
}

// ---------------------------------------------------------------------------
// CollectionChange / CollectionInput (public) — unchanged
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
    pub(crate) single_inputs:      Vec<Option<Box<dyn Any + Send + Sync>>>,
    pub(crate) collection_inputs:  Vec<Option<ErasedCollection>>,
    pub(crate) slot_is_collection: Vec<bool>,
    pub(crate) outputs:            Vec<Option<ContextOutput>>,
    pub(crate) layout:             Arc<SlotLayout>,
    pub(crate) dispatch:           Arc<DispatchTable>,
    pub(crate) registry:           Arc<crate::value::ValueTypeRegistry>,
}

impl TransformContext {
    pub(crate) fn new(
        layout:   Arc<SlotLayout>,
        dispatch: Arc<DispatchTable>,
        registry: Arc<crate::value::ValueTypeRegistry>,
    ) -> Self {
        let n_in  = layout.inputs.len();
        let n_out = layout.outputs.len();
        let slot_is_collection = layout.inputs.iter().map(|s| s.is_col).collect();
        Self {
            single_inputs:      (0..n_in).map(|_| None).collect(),
            collection_inputs:  (0..n_in).map(|_| None).collect(),
            slot_is_collection,
            outputs:            (0..n_out).map(|_| None).collect(),
            layout,
            dispatch,
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
        let spec = &self.layout.outputs[slot];
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
        let spec = &self.layout.outputs[slot];
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
        let extract_key = self.dispatch.outputs.get(slot)
            .and_then(|d| d.extract_key)
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
    /// Declare this transform's slot layout.
    /// Called once at build time; use `ctx.input::<T>()` etc. to declare slots.
    fn register(ctx: &mut impl TransformRegisterContext) where Self: Sized;

    /// Execute one invocation.  Use `?` to propagate `TransformError`.
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError>;
}

// ---------------------------------------------------------------------------
// ErasedTransform (pub(crate)) — object-safe wrapper
// ---------------------------------------------------------------------------

/// Object-safe wrapper holding a `Transform` instance (via `Arc<dyn Transform>`) and its
/// slot layout + dispatch table.  `async_trait` makes `Transform` dyn-compatible.
#[derive(Clone)]
pub(crate) struct ErasedTransform {
    pub(crate) schema:    Arc<SlotLayout>,
    pub(crate) dispatch:  Arc<DispatchTable>,
    pub(crate) instance:  Arc<dyn Transform>,
}

impl ErasedTransform {
    pub(crate) fn new<T: Transform>(layout: SlotLayout, dispatch: DispatchTable, instance: T) -> Self {
        Self {
            schema:   Arc::new(layout),
            dispatch: Arc::new(dispatch),
            instance: Arc::new(instance),
        }
    }

    pub(crate) async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        self.instance.apply(ctx).await
    }
}

impl std::fmt::Debug for ErasedTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ErasedTransform({}→{})", self.schema.inputs.len(), self.schema.outputs.len())
    }
}
