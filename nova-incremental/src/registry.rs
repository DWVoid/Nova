//! Transform registry: name → [`Transform`] mapping.  Internal only.
//!
//! Users register transforms exclusively via [`crate::engine::IncrementalEngine`]
//! typed registration methods.  `TransformRegistry` is `pub(crate)` and never
//! exposed in the public API.
use std::collections::HashMap;
use crate::transform::Transform;

/// Engine-private registry of named [`Transform`] instances.
#[derive(Default, Clone)]
pub(crate) struct TransformRegistry {
    map: HashMap<String, Transform>,
}

impl TransformRegistry {
    pub(crate) fn new() -> Self {
        Self { map: HashMap::new() }
    }

    pub(crate) fn register(&mut self, key: impl Into<String>, transform: Transform) {
        self.map.insert(key.into(), transform);
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Transform> {
        self.map.get(key)
    }

    #[allow(dead_code)]
    pub(crate) fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::{Transform, TypedOneToOne};
    use crate::value::ValueTypeRegistry;
    use std::sync::Arc;

    fn make_noop_transform() -> Transform {
        let mut r = ValueTypeRegistry::new();
        r.register_primitives().unwrap();
        let reg = Arc::new(r);
        Transform::OneToOne(Arc::new(TypedOneToOne::new(
            |n: &i32| { let n = *n; async move { Ok(n) } },
            reg,
        )))
    }

    #[test]
    fn register_and_get() {
        let mut reg = TransformRegistry::new();
        reg.register("noop", make_noop_transform());
        assert!(reg.get("noop").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn contains_returns_correct_result() {
        let mut reg = TransformRegistry::new();
        reg.register("x", make_noop_transform());
        assert!(reg.contains("x"));
        assert!(!reg.contains("y"));
    }

    #[test]
    fn register_overwrites() {
        let mut reg = TransformRegistry::new();
        reg.register("k", make_noop_transform());
        reg.register("k", make_noop_transform());
        assert!(reg.get("k").is_some());
    }
}