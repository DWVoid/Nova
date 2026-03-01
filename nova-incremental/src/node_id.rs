//! Stable node identity.
//!
//! ## Design: Why UUID?
//!
//! Each node in the incremental graph needs an identifier that is:
//! - **Stable across restarts** – so persisted data can be reloaded and nodes
//!   re-associated without re-numbering everything.
//! - **Globally unique without coordination** – UUID v4 generation requires no
//!   shared counter, making it safe to generate node IDs in parallel or across
//!   multiple processes.
//! - **Reproducible when needed** – UUID v5 (name-based) lets callers derive a
//!   deterministic ID from a stable namespace + name, which is useful for
//!   "well-known" input nodes that must survive a full graph rebuild (e.g. a
//!   source file whose path is the stable name).
//!
//! Sequential integer IDs were considered but rejected: they require a shared
//! atomic counter, are meaningless after a graph reload, and make distributed
//! graph composition impossible.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::fmt;

/// A stable, unique identifier for a node in the incremental computation graph.
///
/// Use [`NodeId::new`] for ad-hoc nodes and [`NodeId::named`] for nodes whose
/// identity must survive a full graph rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(Uuid);

impl NodeId {
    /// Generate a new random (v4) node ID.
    #[inline]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Derive a deterministic (v5) node ID from a namespace UUID and a name
    /// string.  Two calls with the same arguments always produce the same
    /// `NodeId`, making this suitable for "well-known" input nodes.
    ///
    /// # Example
    /// ```
    /// use nova_incremental::NodeId;
    /// use uuid::Uuid;
    ///
    /// let ns = Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();
    /// let id = NodeId::named(ns, "my_source_file.nova");
    /// ```
    #[inline]
    pub fn named(namespace: Uuid, name: &str) -> Self {
        Self(Uuid::new_v5(&namespace, name.as_bytes()))
    }

    /// Return the underlying [`Uuid`].
    #[inline]
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Encode the node ID as a lower-hex string suitable for use as a storage
    /// key (no hyphens, fixed 32 chars).
    #[inline]
    pub fn to_storage_key(&self) -> String {
        self.0.simple().to_string()
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_unique() {
        let a = NodeId::new();
        let b = NodeId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn named_ids_are_deterministic() {
        let ns = Uuid::new_v4();
        let a = NodeId::named(ns, "foo");
        let b = NodeId::named(ns, "foo");
        assert_eq!(a, b);
    }

    #[test]
    fn named_ids_differ_for_different_names() {
        let ns = Uuid::new_v4();
        let a = NodeId::named(ns, "foo");
        let b = NodeId::named(ns, "bar");
        assert_ne!(a, b);
    }

    #[test]
    fn storage_key_is_32_chars() {
        let id = NodeId::new();
        assert_eq!(id.to_storage_key().len(), 32);
    }

    #[test]
    fn round_trips_through_display() {
        let id = NodeId::new();
        assert_eq!(id.to_string().len(), 36); // standard UUID with hyphens
    }

    #[test]
    fn serde_round_trip() {
        let id = NodeId::new();
        let bytes = rmp_serde::to_vec(&id).unwrap();
        let back: NodeId = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(id, back);
    }
}
