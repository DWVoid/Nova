//! Transform registry: name → [`Transform`] mapping.
//!
//! ## Design: Separating Transform Registration from Graph Topology
//!
//! Transforms are Rust functions (code), not data.  They cannot be serialised.
//! When a graph is restored from persistent storage, only the *name* of each
//! transform is known.  The user must re-supply the actual function objects by
//! calling [`TransformRegistry::register`] with the same key strings that were
//! used when the graph was first built.
use std::collections::HashMap;
use crate::incremental::transform::Transform;
/// A named registry of [`Transform`] instances.
#[derive(Default, Clone)]
pub struct TransformRegistry {
    map: HashMap<String, Transform>,
}
impl TransformRegistry {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }
    /// Register a transform under `key`, replacing any existing entry.
    pub fn register(&mut self, key: impl Into<String>, transform: Transform) {
        self.map.insert(key.into(), transform);
    }
    /// Look up a transform by key.
    pub fn get(&self, key: &str) -> Option<&Transform> {
        self.map.get(key)
    }
    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Transform)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::transform::{Transform, OneToOneTransform, TransformError};
    use crate::incremental::value::Value;
    use async_trait::async_trait;
    use std::sync::Arc;
    struct Noop;
    #[async_trait]
    impl OneToOneTransform for Noop {
        async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
            Ok(input.clone())
        }
    }
    #[test]
    fn register_and_get() {
        let mut reg = TransformRegistry::new();
        reg.register("noop", Transform::OneToOne(Arc::new(Noop)));
        assert!(reg.get("noop").is_some());
        assert!(reg.get("missing").is_none());
    }
    #[test]
    fn contains_returns_correct_result() {
        let mut reg = TransformRegistry::new();
        reg.register("x", Transform::OneToOne(Arc::new(Noop)));
        assert!(reg.contains("x"));
        assert!(!reg.contains("y"));
    }
    #[test]
    fn register_overwrites() {
        let mut reg = TransformRegistry::new();
        reg.register("k", Transform::OneToOne(Arc::new(Noop)));
        reg.register("k", Transform::OneToOne(Arc::new(Noop)));
        assert!(reg.get("k").is_some());
    }
    #[test]
    fn iter_yields_all_entries() {
        let mut reg = TransformRegistry::new();
        reg.register("a", Transform::OneToOne(Arc::new(Noop)));
        reg.register("b", Transform::OneToOne(Arc::new(Noop)));
        let keys: Vec<_> = reg.iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), 2);
    }
}
