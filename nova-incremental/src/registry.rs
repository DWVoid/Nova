//! Transform registry.  Maps string key → `ErasedTransform`.  All `pub(crate)`.
//!
//! Users register transforms exclusively via [`crate::engine::IncrementalEngine`]
//! `register_transform` method.  `TransformRegistry` is `pub(crate)` and never
//! exposed in the public API.
use std::collections::HashMap;
use crate::transform::ErasedTransform;

pub(crate) struct TransformRegistry {
    map: HashMap<String, ErasedTransform>,
}

impl TransformRegistry {
    pub(crate) fn new() -> Self { Self { map: HashMap::new() } }

    pub(crate) fn register(&mut self, key: &str, t: ErasedTransform) {
        self.map.insert(key.to_owned(), t);
    }

    pub(crate) fn get(&self, key: &str) -> Option<&ErasedTransform> { self.map.get(key) }

    pub(crate) fn contains(&self, key: &str) -> bool { self.map.contains_key(key) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::{Transform as TransformTrait, TransformSchema,
                           TransformContext, TransformError, ErasedTransform};
    use async_trait::async_trait;

    struct Noop;
    #[async_trait]
    impl TransformTrait for Noop {
        fn schema() -> TransformSchema where Self: Sized {
            TransformSchema::new().input::<i32>().output::<i32>()
        }
        async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
            let v = *ctx.input::<i32>(0)?;
            ctx.output(0, v)
        }
    }

    fn make_erased() -> ErasedTransform {
        ErasedTransform::new(Noop::schema(), Noop)
    }

    #[test]
    fn register_and_get() {
        let mut reg = TransformRegistry::new();
        reg.register("noop", make_erased());
        assert!(reg.get("noop").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn contains() {
        let mut reg = TransformRegistry::new();
        reg.register("x", make_erased());
        assert!(reg.contains("x"));
        assert!(!reg.contains("y"));
    }

    #[test]
    fn overwrite() {
        let mut reg = TransformRegistry::new();
        reg.register("k", make_erased());
        reg.register("k", make_erased());
        assert!(reg.get("k").is_some());
    }
}