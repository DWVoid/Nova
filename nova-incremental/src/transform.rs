//! Transform function abstractions – internal implementation detail.
//!
//! Users register transforms through [`crate::engine::IncrementalEngine`]
//! typed registration methods (`register_one_to_one`, etc.).  The `Transform`
//! enum and arity traits are `pub(crate)` and never exposed in the public API.
//!
//! ## Design: Typed Closure Wrappers
//!
//! Each typed registration method (`register_one_to_one<In, Out, F, Fut>`)
//! wraps the user-supplied closure in a `TypedOneToOne` adapter that:
//! 1. Decodes `Value` → `In` via the `ValueTypeRegistry`.
//! 2. Calls the user closure.
//! 3. Encodes `Out` → `Value` via the `ValueTypeRegistry`.
//!
//! This keeps `Value` entirely hidden from user code.

use std::sync::Arc;
use std::future::Future;
use async_trait::async_trait;
use crate::value::{Value, ValueTypeRegistry, RegistryError};

// ---------------------------------------------------------------------------
// Public: TransformError
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
        if let Some(src) = &self.source {
            write!(f, ": {}", src)?;
        }
        Ok(())
    }
}

impl std::error::Error for TransformError {}

impl From<RegistryError> for TransformError {
    fn from(e: RegistryError) -> Self {
        TransformError::new(e.message)
    }
}

// ---------------------------------------------------------------------------
// Internal: arity traits
// ---------------------------------------------------------------------------

#[async_trait]
pub(crate) trait OneToOneTransform: Send + Sync {
    async fn apply(&self, input: &Value) -> Result<Value, TransformError>;
}

#[async_trait]
pub(crate) trait OneToManyTransform: Send + Sync {
    async fn apply(&self, input: &Value) -> Result<Vec<Value>, TransformError>;
}

#[async_trait]
pub(crate) trait ManyToOneTransform: Send + Sync {
    async fn apply(&self, inputs: &[Value]) -> Result<Value, TransformError>;
}

#[async_trait]
pub(crate) trait ManyToManyTransform: Send + Sync {
    async fn apply(&self, inputs: &[Value]) -> Result<Vec<Value>, TransformError>;
}

// ---------------------------------------------------------------------------
// Internal: Transform enum
// ---------------------------------------------------------------------------

/// Unified transform handle stored on each graph edge.  Internal only.
#[derive(Clone)]
pub(crate) enum Transform {
    OneToOne(Arc<dyn OneToOneTransform>),
    OneToMany(Arc<dyn OneToManyTransform>),
    ManyToOne(Arc<dyn ManyToOneTransform>),
    ManyToMany(Arc<dyn ManyToManyTransform>),
}

impl Transform {
    pub(crate) async fn apply(&self, inputs: &[Value]) -> Result<Vec<Value>, TransformError> {
        match self {
            Transform::OneToOne(t) => {
                debug_assert_eq!(inputs.len(), 1, "OneToOne expects exactly 1 input");
                Ok(vec![t.apply(&inputs[0]).await?])
            }
            Transform::OneToMany(t) => {
                debug_assert_eq!(inputs.len(), 1, "OneToMany expects exactly 1 input");
                t.apply(&inputs[0]).await
            }
            Transform::ManyToOne(t) => {
                Ok(vec![t.apply(inputs).await?])
            }
            Transform::ManyToMany(t) => {
                t.apply(inputs).await
            }
        }
    }

    pub(crate) fn arity_name(&self) -> &'static str {
        match self {
            Transform::OneToOne(_)  => "OneToOne",
            Transform::OneToMany(_) => "OneToMany",
            Transform::ManyToOne(_) => "ManyToOne",
            Transform::ManyToMany(_) => "ManyToMany",
        }
    }
}

impl std::fmt::Debug for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Transform::{}", self.arity_name())
    }
}

// ---------------------------------------------------------------------------
// Internal: typed closure adapters
// ---------------------------------------------------------------------------

/// Adapter that wraps a user `Fn(&In) -> Fut` and handles Value encode/decode.
pub(crate) struct TypedOneToOne<In, Out, F, Fut>
where
    In:  Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    pub(crate) f: F,
    pub(crate) registry: Arc<ValueTypeRegistry>,
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
        Self { f, registry, _in: std::marker::PhantomData, _out: std::marker::PhantomData }
    }
}

use std::any::Any;

#[async_trait]
impl<In, Out, F, Fut> OneToOneTransform for TypedOneToOne<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
        let typed_in = self.registry.downcast_value::<In>(input, "OneToOne input")?;
        let typed_out = (self.f)(&typed_in).await?;
        self.registry.make_value(typed_out).map_err(TransformError::from)
    }
}

/// Adapter for `Fn(&[In]) -> Fut` → one output.
pub(crate) struct TypedManyToOne<In, Out, F, Fut>
where
    In:  Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    pub(crate) f: F,
    pub(crate) registry: Arc<ValueTypeRegistry>,
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
        Self { f, registry, _in: std::marker::PhantomData, _out: std::marker::PhantomData }
    }
}

#[async_trait]
impl<In, Out, F, Fut> ManyToOneTransform for TypedManyToOne<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
{
    async fn apply(&self, inputs: &[Value]) -> Result<Value, TransformError> {
        let typed_ins: Result<Vec<In>, _> = inputs.iter().enumerate().map(|(i, v)| {
            self.registry.downcast_value::<In>(v, &format!("ManyToOne input[{i}]"))
        }).collect();
        let typed_out = (self.f)(&typed_ins?).await?;
        self.registry.make_value(typed_out).map_err(TransformError::from)
    }
}

/// Adapter for `Fn(&In) -> Fut` → many outputs.
pub(crate) struct TypedOneToMany<In, Out, F, Fut>
where
    In:  Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    pub(crate) f: F,
    pub(crate) registry: Arc<ValueTypeRegistry>,
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
        Self { f, registry, _in: std::marker::PhantomData, _out: std::marker::PhantomData }
    }
}

#[async_trait]
impl<In, Out, F, Fut> OneToManyTransform for TypedOneToMany<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&In) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    async fn apply(&self, input: &Value) -> Result<Vec<Value>, TransformError> {
        let typed_in = self.registry.downcast_value::<In>(input, "OneToMany input")?;
        let typed_outs = (self.f)(&typed_in).await?;
        typed_outs.into_iter().map(|v| {
            self.registry.make_value(v).map_err(TransformError::from)
        }).collect()
    }
}

/// Adapter for `Fn(&[In]) -> Fut` → many outputs.
pub(crate) struct TypedManyToMany<In, Out, F, Fut>
where
    In:  Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    pub(crate) f: F,
    pub(crate) registry: Arc<ValueTypeRegistry>,
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
        Self { f, registry, _in: std::marker::PhantomData, _out: std::marker::PhantomData }
    }
}

#[async_trait]
impl<In, Out, F, Fut> ManyToManyTransform for TypedManyToMany<In, Out, F, Fut>
where
    In:  Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    Out: Any + Send + Sync + Clone + serde::Serialize + serde::de::DeserializeOwned + 'static,
    F:  Fn(&[In]) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
{
    async fn apply(&self, inputs: &[Value]) -> Result<Vec<Value>, TransformError> {
        let typed_ins: Result<Vec<In>, _> = inputs.iter().enumerate().map(|(i, v)| {
            self.registry.downcast_value::<In>(v, &format!("ManyToMany input[{i}]"))
        }).collect();
        let typed_outs = (self.f)(&typed_ins?).await?;
        typed_outs.into_iter().map(|v| {
            self.registry.make_value(v).map_err(TransformError::from)
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::ValueTypeRegistry;
    use std::sync::Arc;

    fn make_registry() -> Arc<ValueTypeRegistry> {
        let mut r = ValueTypeRegistry::new();
        r.register_primitives().unwrap();
        Arc::new(r)
    }

    #[tokio::test]
    async fn typed_one_to_one_doubles() {
        let reg = make_registry();
        let t = Transform::OneToOne(Arc::new(TypedOneToOne::new(
            |n: &i32| {
                let n = *n;
                async move { Ok(n * 2) }
            },
            Arc::clone(&reg),
        )));
        let input = reg.make_value(5i32).unwrap();
        let out = t.apply(&[input]).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].downcast::<i32>(), Some(&10i32));
    }

    #[tokio::test]
    async fn typed_many_to_one_sums() {
        let reg = make_registry();
        let t = Transform::ManyToOne(Arc::new(TypedManyToOne::new(
            |inputs: &[i32]| {
                let sum: i32 = inputs.iter().sum();
                async move { Ok(sum) }
            },
            Arc::clone(&reg),
        )));
        let a = reg.make_value(3i32).unwrap();
        let b = reg.make_value(4i32).unwrap();
        let out = t.apply(&[a, b]).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].downcast::<i32>(), Some(&7i32));
    }

    #[tokio::test]
    async fn typed_one_to_many_duplicates() {
        let reg = make_registry();
        let t = Transform::OneToMany(Arc::new(TypedOneToMany::new(
            |n: &i32| {
                let n = *n;
                async move { Ok(vec![n, n]) }
            },
            Arc::clone(&reg),
        )));
        let input = reg.make_value(7i32).unwrap();
        let out = t.apply(&[input]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].downcast::<i32>(), Some(&7i32));
        assert_eq!(out[1].downcast::<i32>(), Some(&7i32));
    }

    #[tokio::test]
    async fn type_mismatch_produces_transform_error() {
        let reg = make_registry();
        let t = Transform::OneToOne(Arc::new(TypedOneToOne::new(
            |n: &i32| { let n = *n; async move { Ok(n * 2) } },
            Arc::clone(&reg),
        )));
        // Pass u64 instead of i32
        let input = reg.make_value(5u64).unwrap();
        let result = t.apply(&[input]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("type mismatch"));
    }
}