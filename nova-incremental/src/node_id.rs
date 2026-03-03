//! Stable `NodeId` — `pub(crate)` newtype over `Uuid`.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Internal stable node identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct NodeId(pub(crate) Uuid);

impl NodeId {
    pub(crate) fn new() -> Self { Self(Uuid::new_v4()) }
    pub(crate) fn from_uuid(u: Uuid) -> Self { Self(u) }
    pub(crate) fn as_uuid(self) -> Uuid { self.0 }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.simple())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn uniqueness() { assert_ne!(NodeId::new(), NodeId::new()); }
    #[test] fn round_trip() {
        let id = NodeId::new();
        assert_eq!(NodeId::from_uuid(id.as_uuid()), id);
    }
}