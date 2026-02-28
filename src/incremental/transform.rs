//! Transform function abstractions for all four arities.
//!
//! ## Design: Four Arity Traits
//!
//! Transforms in the incremental graph connect source nodes to target nodes.
//! Four shapes cover the practical cases:
//!
//! | Arity       | Sources | Targets | Example use-case                         |
//! |-------------|---------|---------|------------------------------------------|
//! | `OneToOne`  | 1       | 1       | Type-check a single expression           |
//! | `OneToMany` | 1       | N       | Split a token stream into declarations   |
//! | `ManyToOne` | N       | 1       | Link N modules into a single IR          |
//! | `ManyToMany`| N       | M       | Desugar pass that emits N→M items        |
//!
//! All four traits are **async** via [`async_trait`] so transforms can await
//! I/O (e.g. reading additional source files through a VFS).
//!
//! ## Design: `Transform` Enum vs. Trait Object
//!
//! We store transforms as an enum that holds `Arc<dyn Trait>` for each arity.
//! The enum dispatch adds one branch at call-time, but it allows:
//! - Uniform storage in `EdgeEntry` without extra boxing.
//! - Static dispatch of the arity at scheduling time (the scheduler needs to
//!   know how many inputs/outputs an edge has to wire `Value` slices correctly).
//! - Cheap cloning of edge handles (`Arc` clone = pointer copy).
//!
//! ## Design: Error Handling
//!
//! Transforms return `Result<_, TransformError>`.  A failed transform leaves
//! the target nodes in the `NodeStatus::Error` state (see `graph.rs`) so the
//! scheduler does not retry them on the next `update()` call unless the user
//! explicitly clears the error or changes an input.  This avoids infinite
//! retry loops for deterministically failing transforms.

use std::sync::Arc;
use async_trait::async_trait;
use crate::incremental::value::Value;

/// Error produced by a transform function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformError {
    /// Human-readable description of the failure.
    pub message: String,
    /// Optional lower-level error message (e.g. from an I/O error).
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

// ---------------------------------------------------------------------------
// Arity traits
// ---------------------------------------------------------------------------

/// A transform that maps exactly one input [`Value`] to one output [`Value`].
#[async_trait]
pub trait OneToOneTransform: Send + Sync {
    async fn apply(&self, input: &Value) -> Result<Value, TransformError>;
}

/// A transform that maps exactly one input [`Value`] to multiple output
/// [`Value`]s.  The returned `Vec` must have length equal to the number of
/// target nodes declared for the edge.
#[async_trait]
pub trait OneToManyTransform: Send + Sync {
    async fn apply(&self, input: &Value) -> Result<Vec<Value>, TransformError>;
}

/// A transform that combines multiple input [`Value`]s into one output
/// [`Value`].  The `inputs` slice is ordered the same as the `sources` list on
/// the edge.
#[async_trait]
pub trait ManyToOneTransform: Send + Sync {
    async fn apply(&self, inputs: &[Value]) -> Result<Value, TransformError>;
}

/// A transform that maps multiple input [`Value`]s to multiple output
/// [`Value`]s.  The returned `Vec` must have length equal to the number of
/// target nodes declared for the edge.
#[async_trait]
pub trait ManyToManyTransform: Send + Sync {
    async fn apply(&self, inputs: &[Value]) -> Result<Vec<Value>, TransformError>;
}

// ---------------------------------------------------------------------------
// Uniform enum wrapper
// ---------------------------------------------------------------------------

/// Unified transform handle stored on each graph edge.
///
/// Wraps one of the four arity variants behind an `Arc` for cheap cloning.
#[derive(Clone)]
pub enum Transform {
    OneToOne(Arc<dyn OneToOneTransform>),
    OneToMany(Arc<dyn OneToManyTransform>),
    ManyToOne(Arc<dyn ManyToOneTransform>),
    ManyToMany(Arc<dyn ManyToManyTransform>),
}

impl Transform {
    /// Apply this transform to the provided inputs and return a `Vec` of
    /// output values (always `Vec` for uniformity; arity-1 cases return
    /// a single-element vec).
    ///
    /// Panics in debug builds if the wrong number of inputs is provided.
    pub async fn apply(&self, inputs: &[Value]) -> Result<Vec<Value>, TransformError> {
        match self {
            Transform::OneToOne(t) => {
                debug_assert_eq!(inputs.len(), 1, "OneToOne expects exactly 1 input");
                let out = t.apply(&inputs[0]).await?;
                Ok(vec![out])
            }
            Transform::OneToMany(t) => {
                debug_assert_eq!(inputs.len(), 1, "OneToMany expects exactly 1 input");
                t.apply(&inputs[0]).await
            }
            Transform::ManyToOne(t) => {
                let out = t.apply(inputs).await?;
                Ok(vec![out])
            }
            Transform::ManyToMany(t) => {
                t.apply(inputs).await
            }
        }
    }

    /// Declare what kind of transform this is (for diagnostics / scheduling).
    pub fn arity_name(&self) -> &'static str {
        match self {
            Transform::OneToOne(_) => "OneToOne",
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Double;
    #[async_trait]
    impl OneToOneTransform for Double {
        async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
            let n = input.downcast::<i32>().ok_or_else(|| TransformError::new("expected i32"))?;
            Ok(Value::new(*n * 2))
        }
    }

    struct Sum;
    #[async_trait]
    impl ManyToOneTransform for Sum {
        async fn apply(&self, inputs: &[Value]) -> Result<Value, TransformError> {
            let mut total = 0i32;
            for v in inputs {
                total += v.downcast::<i32>().ok_or_else(|| TransformError::new("expected i32"))?;
            }
            Ok(Value::new(total))
        }
    }

    struct Split;
    #[async_trait]
    impl OneToManyTransform for Split {
        async fn apply(&self, input: &Value) -> Result<Vec<Value>, TransformError> {
            let n = input.downcast::<i32>().ok_or_else(|| TransformError::new("expected i32"))?;
            Ok(vec![Value::new(*n), Value::new(*n + 1)])
        }
    }

    #[tokio::test]
    async fn one_to_one_doubles() {
        let t = Transform::OneToOne(Arc::new(Double));
        let out = t.apply(&[Value::new(5i32)]).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].downcast::<i32>(), Some(&10i32));
    }

    #[tokio::test]
    async fn many_to_one_sums() {
        let t = Transform::ManyToOne(Arc::new(Sum));
        let out = t.apply(&[Value::new(3i32), Value::new(4i32)]).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].downcast::<i32>(), Some(&7i32));
    }

    #[tokio::test]
    async fn one_to_many_splits() {
        let t = Transform::OneToMany(Arc::new(Split));
        let out = t.apply(&[Value::new(10i32)]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].downcast::<i32>(), Some(&10i32));
        assert_eq!(out[1].downcast::<i32>(), Some(&11i32));
    }

    #[tokio::test]
    async fn transform_error_propagates() {
        let t = Transform::OneToOne(Arc::new(Double));
        // Pass wrong type (u64 instead of i32)
        let result = t.apply(&[Value::new(99u64)]).await;
        assert!(result.is_err());
    }
}
