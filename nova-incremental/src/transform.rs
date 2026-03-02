//! Transform function abstractions.
//!
//! ## Design: Single Trait, Arbitrary Slots
//!
//! The old four-arity enum (`OneToOne`, `ManyToOne`, etc.) is replaced by a
//! single [`TransformFn`] trait.  Every transform declares a
//! [`TransformSchema`] (arbitrary number of typed input/output slots) and an
//! `apply` method that receives [`SlotInput`]s and returns [`SlotOutput`]s.
//!
//! ## Design: `prev_output` for Cycle Caching
//!
//! Transforms that participate in a legal cycle receive their previous output
//! via `prev_output: Option<&[SlotOutput]>`.  This is an **in-memory hint**
//!" only — `None` on first run and after a restart.
//!
//! ## Design: Crossing-Kind Edges
//!
//! - **`Collection→Single`**: scheduler invokes the transform once per element.
//! - **`Single→Collection`**: scheduler inserts the value into the gather slot.

use std::any::Any;
use std::future::Future;
use std::sync::Arc;
use async_trait::async_trait;
use crate::collection::{CollectionDiff, ElementKey};
use crate::slot::TransformSchema;
use crate::value::{Value, ValueTypeRegistry, RegistryError};

// ---------------------------------------------------------------------------
// TransformError (public)
// ---------------------------------------------------------------------------

/// Error produced by a transform function.
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
        if let Some(src) = &self.source { write!(f, ": {src}")?; }
        Ok(())
    }
}
impl std::error::Error for TransformError {}
impl From<RegistryError> for TransformError {
    fn from(e: RegistryError) -> Self { TransformError::new(e.message) }
}

// ---------------------------------------------------------------------------
// SlotInput / SlotOutput
// ---------------------------------------------------------------------------

/// Value delivered to one input slot during a transform invocation.
#[derive(Clone, Debug)]
pub enum SlotInput {
    /// A single value for a `Single`-typed input slot.
    Single(Value),
    /// An ordered element list + diff for a `Collection`-typed input slot.
    Collection {
        elements: Vec<Value>,
        diff: CollectionDiff,
    },
}

/// Value produced for one output slot by a transform invocation.
#[derive(Clone, Debug)]
pub enum SlotOutput {
    /// A single value for a `Single`-typed output slot.
    Single(Value),
    /// `(element_key, value)` pairs for a `Collection`-typed output slot.
    Collection(Vec<(ElementKey, Value)>),
}

// ---------------------------------------------------------------------------
// TransformFn trait
// ---------------------------------------------------------------------------

#[async_trait]
pub(crate) trait TransformFn: Send + Sync {
    fn schema(&self) -> &TransformSchema;
    async fn apply(
        &self,
        inputs: &[SlotInput],
        prev_output: Option<&[SlotOutput]>,
    ) -> Result<Vec<SlotOutput>, TransformError>;
}

// ---------------------------------------------------------------------------
// Transform handle
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct Transform {
    pub(crate) f: Arc<dyn TransformFn>,
}

impl Transform {
    pub(crate) fn new(f: Arc<dyn TransformFn>) -> Self { Self { f } }
    pub(crate) fn schema(&self) -> &TransformSchema { self.f.schema() }
    pub(crate) async fn apply(
        &self,
        inputs: &[SlotInput],
        prev_output: Option<&[SlotOutput]>,
    ) -> Result<Vec<SlotOutput>, TransformError> {
        self.f.apply(inputs, prev_output).await
    }
}

impl std::fmt::Debug for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Transform({} in → {} out)",
            self.schema().inputs.len(), self.schema().outputs.len())
    }
}

// ---------------------------------------------------------------------------
// Typed closure adapters
// ---------------------------------------------------------------------------

/// `Fn(&In) -> Fut` → `Single(Out)`.
pub(crate) struct TypedOneToOne<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    pub(crate) f: F,
    pub(crate) registry: Arc<ValueTypeRegistry>,
    pub(crate) schema: TransformSchema,
    _in: std::marker::PhantomData<In>,
    _out: std::marker::PhantomData<Out>,
}

impl<In, Out, F, Fut> TypedOneToOne<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    pub(crate) fn new(f: F, registry: Arc<ValueTypeRegistry>) -> Self {
        let in_key  = registry.key_for_type_id(std::any::TypeId::of::<In>())
            .unwrap_or_else(|| std::any::type_name::<In>().to_string());
        let out_key = registry.key_for_type_id(std::any::TypeId::of::<Out>())
            .unwrap_or_else(|| std::any::type_name::<Out>().to_string());
        Self {
            f, registry,
            schema: TransformSchema::one_to_one(in_key, out_key),
            _in: std::marker::PhantomData,
            _out: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<In, Out, F, Fut> TransformFn for TypedOneToOne<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    fn schema(&self) -> &TransformSchema { &self.schema }
    async fn apply(&self, inputs: &[SlotInput], _prev: Option<&[SlotOutput]>) -> Result<Vec<SlotOutput>, TransformError> {
        let v = match &inputs[0] {
            SlotInput::Single(v) => v,
            _ => return Err(TransformError::new("OneToOne: expected Single input")),
        };
        let typed: In = self.registry.downcast_value::<In>(v)
            .map_err(|e| TransformError::new(format!("OneToOne input: {}", e.message)))?;
        let out: Out = (self.f)(&typed).await?;
        let out_val = self.registry.make_value(out).map_err(TransformError::from)?;
        Ok(vec![SlotOutput::Single(out_val)])
    }
}

/// `Fn(&[In]) -> Fut<Out>` — N single inputs → 1 output.
pub(crate) struct TypedManyToOne<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    pub(crate) f: F,
    pub(crate) registry: Arc<ValueTypeRegistry>,
    pub(crate) schema: TransformSchema,
    _in: std::marker::PhantomData<In>,
    _out: std::marker::PhantomData<Out>,
}

impl<In, Out, F, Fut> TypedManyToOne<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    pub(crate) fn new(f: F, registry: Arc<ValueTypeRegistry>) -> Self {
        let in_key  = registry.key_for_type_id(std::any::TypeId::of::<In>())
            .unwrap_or_else(|| std::any::type_name::<In>().to_string());
        let out_key = registry.key_for_type_id(std::any::TypeId::of::<Out>())
            .unwrap_or_else(|| std::any::type_name::<Out>().to_string());
        // Schema is fixed at registration; many-to-one compat shim uses 0 inputs
        // (actual input count is determined by wiring).
        let schema = TransformSchema { inputs: vec![], outputs: vec![crate::slot::SlotDescriptor::single(out_key)] };
        let _ = in_key; // stored for documentation; inputs wired dynamically
        Self { f, registry, schema, _in: std::marker::PhantomData, _out: std::marker::PhantomData }
    }
}

#[async_trait]
impl<In, Out, F, Fut> TransformFn for TypedManyToOne<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    fn schema(&self) -> &TransformSchema { &self.schema }
    async fn apply(&self, inputs: &[SlotInput], _prev: Option<&[SlotOutput]>) -> Result<Vec<SlotOutput>, TransformError> {
        let typed: Result<Vec<In>, _> = inputs.iter().enumerate().map(|(i, s)| {
            match s {
                SlotInput::Single(v) => self.registry.downcast_value::<In>(v)
                    .map_err(|e| TransformError::new(format!("ManyToOne input[{i}]: {}", e.message))),
                _ => Err(TransformError::new(format!("ManyToOne input[{i}]: expected Single"))),
            }
        }).collect();
        let out = (self.f)(&typed?).await?;
        Ok(vec![SlotOutput::Single(self.registry.make_value(out).map_err(TransformError::from)?)])
    }
}

/// `Fn(&In) -> Fut<Vec<Out>>` — 1 input → N outputs.
pub(crate) struct TypedOneToMany<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    pub(crate) f: F,
    pub(crate) registry: Arc<ValueTypeRegistry>,
    pub(crate) schema: TransformSchema,
    _in: std::marker::PhantomData<In>,
    _out: std::marker::PhantomData<Out>,
}

impl<In, Out, F, Fut> TypedOneToMany<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    pub(crate) fn new(f: F, registry: Arc<ValueTypeRegistry>) -> Self {
        let in_key  = registry.key_for_type_id(std::any::TypeId::of::<In>())
            .unwrap_or_else(|| std::any::type_name::<In>().to_string());
        let out_key = registry.key_for_type_id(std::any::TypeId::of::<Out>())
            .unwrap_or_else(|| std::any::type_name::<Out>().to_string());
        // Output count unknown at registration; schema updated when outputs are wired.
        let schema = TransformSchema { inputs: vec![crate::slot::SlotDescriptor::single(in_key)], outputs: vec![] };
        let _ = out_key;
        Self { f, registry, schema, _in: std::marker::PhantomData, _out: std::marker::PhantomData }
    }
}

#[async_trait]
impl<In, Out, F, Fut> TransformFn for TypedOneToMany<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    fn schema(&self) -> &TransformSchema { &self.schema }
    async fn apply(&self, inputs: &[SlotInput], _prev: Option<&[SlotOutput]>) -> Result<Vec<SlotOutput>, TransformError> {
        let v = match &inputs[0] {
            SlotInput::Single(v) => v,
            _ => return Err(TransformError::new("OneToMany: expected Single input")),
        };
        let typed: In = self.registry.downcast_value::<In>(v)
            .map_err(|e| TransformError::new(format!("OneToMany input: {}", e.message)))?;
        let outs: Vec<Out> = (self.f)(&typed).await?;
        outs.into_iter().map(|o| {
            self.registry.make_value(o).map(SlotOutput::Single).map_err(TransformError::from)
        }).collect()
    }
}

/// `Fn(&[In]) -> Fut<Vec<Out>>` — N inputs → M outputs.
pub(crate) struct TypedManyToMany<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    pub(crate) f: F,
    pub(crate) registry: Arc<ValueTypeRegistry>,
    pub(crate) schema: TransformSchema,
    _in: std::marker::PhantomData<In>,
    _out: std::marker::PhantomData<Out>,
}

impl<In, Out, F, Fut> TypedManyToMany<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    pub(crate) fn new(f: F, registry: Arc<ValueTypeRegistry>) -> Self {
        let schema = TransformSchema { inputs: vec![], outputs: vec![] };
        Self { f, registry, schema, _in: std::marker::PhantomData, _out: std::marker::PhantomData }
    }
}

#[async_trait]
impl<In, Out, F, Fut> TransformFn for TypedManyToMany<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    fn schema(&self) -> &TransformSchema { &self.schema }
    async fn apply(&self, inputs: &[SlotInput], _prev: Option<&[SlotOutput]>) -> Result<Vec<SlotOutput>, TransformError> {
        let typed: Result<Vec<In>, _> = inputs.iter().enumerate().map(|(i, s)| {
            match s {
                SlotInput::Single(v) => self.registry.downcast_value::<In>(v)
                    .map_err(|e| TransformError::new(format!("ManyToMany input[{i}]: {}", e.message))),
                _ => Err(TransformError::new(format!("ManyToMany input[{i}]: expected Single"))),
            }
        }).collect();
        let outs: Vec<Out> = (self.f)(&typed?).await?;
        outs.into_iter().map(|o| {
            self.registry.make_value(o).map(SlotOutput::Single).map_err(TransformError::from)
        }).collect()
    }
}

