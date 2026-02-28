//! Async key-value storage trait and serialisation helpers.
//!
//! ## Design: Storage as a User-Supplied Trait
//!
//! The incremental engine needs durability, but the right storage backend
//! depends entirely on the host application:
//! - A compiler might use a memory-mapped file (fast random access).
//! - An IDE might use an SQLite database (transactional, concurrent reads).
//! - A build system might use a cloud object store (distributed).
//!
//! Rather than baking in a specific backend, we define a minimal async
//! key-value trait and let the user provide the implementation.  The engine
//! only calls `get`, `set`, `delete`, and `contains`; all storage concerns
//! (caching, durability, transactions) are the caller's responsibility.
//!
//! ## Design: MessagePack Encoding
//!
//! We use `rmp-serde` (MessagePack) for serialising [`PersistedNodeData`]:
//! - Compact binary format – smaller than JSON.
//! - Fast encode/decode with no schema compile step (unlike Protocol Buffers).
//! - Already a dependency of the parent project.
//!
//! Raw `Vec<u8>` node values are stored opaque: the engine does not need to
//! understand their content for persistence, only for hash comparison.
//!
//! ## Design: Transforms Are Not Serialised
//!
//! Transform functions are code, not data – they cannot be meaningfully
//! serialised.  Instead, each edge stores a `transform_key` (a
//! user-assigned stable string).  On graph reload, the user re-registers all
//! transforms with the same keys via [`crate::incremental::registry::TransformRegistry`],
//! and the loader re-associates them.  This mirrors the approach taken by
//! incremental compilation frameworks like Salsa.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::incremental::node_id::NodeId;
use crate::incremental::value::ValueHash;

// ---------------------------------------------------------------------------
// Storage key / value newtypes
// ---------------------------------------------------------------------------

/// A string key used to address a record in the key-value store.
///
/// Derived from a [`NodeId`] via [`NodeId::to_storage_key`] for node values,
/// or from a fixed sentinel (e.g. `"__graph_meta__"`) for graph topology.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey(pub String);

impl StorageKey {
    pub fn new(s: impl Into<String>) -> Self { Self(s.into()) }
    pub fn for_node(id: NodeId) -> Self { Self(id.to_storage_key()) }
    pub fn graph_meta() -> Self { Self("__graph_meta__".to_string()) }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl std::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Raw bytes stored under a [`StorageKey`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageValue(pub Vec<u8>);

impl StorageValue {
    pub fn new(bytes: Vec<u8>) -> Self { Self(bytes) }
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
    pub fn into_bytes(self) -> Vec<u8> { self.0 }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// An error returned by a [`Storage`] operation.
#[derive(Debug, Clone)]
pub struct StorageError {
    pub message: String,
    pub source: Option<String>,
}

impl StorageError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), source: None }
    }
    pub fn with_source(message: impl Into<String>, source: impl Into<String>) -> Self {
        Self { message: message.into(), source: Some(source.into()) }
    }
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(src) = &self.source { write!(f, ": {}", src)?; }
        Ok(())
    }
}

impl std::error::Error for StorageError {}

// ---------------------------------------------------------------------------
// Storage trait
// ---------------------------------------------------------------------------

/// Async key-value storage backend.
///
/// Implement this trait on your preferred storage engine and pass it to
/// [`crate::incremental::engine::IncrementalEngine::new`].
///
/// All methods take `&self` (shared reference) so the implementation can use
/// internal mutability (e.g. `Mutex`, `RwLock`, or connection pooling) as
/// needed.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Retrieve the value associated with `key`, or `None` if absent.
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError>;

    /// Store `value` under `key`, overwriting any previous value.
    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError>;

    /// Remove the entry for `key`.  Succeeds silently if the key is absent.
    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError>;

    /// Return `true` if `key` has an associated value.
    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError>;
}

// ---------------------------------------------------------------------------
// Persisted data structures
// ---------------------------------------------------------------------------

/// Per-edge metadata persisted to storage.
///
/// The transform function itself is not serialised; only its `transform_key`
/// is stored so it can be re-associated at reload time via the
/// [`crate::incremental::registry::TransformRegistry`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEdge {
    /// Ordered list of source node IDs.
    pub sources: Vec<NodeId>,
    /// Ordered list of target node IDs.
    pub targets: Vec<NodeId>,
    /// Stable user-assigned name that maps to a concrete [`crate::incremental::transform::Transform`].
    pub transform_key: String,
}

/// All data persisted for a single node.
///
/// `value_bytes` is the MessagePack encoding of the concrete value produced
/// by the node's transform (or supplied directly for input nodes).
///
/// `value_hash` is the hash at the time of last successful computation; used
/// after reload to decide whether an input has changed since the last run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedNodeData {
    pub node_id: NodeId,
    /// MessagePack-encoded concrete value, or empty if never computed.
    pub value_bytes: Vec<u8>,
    /// Hash of the value at last persist time.  Zero if never persisted.
    pub value_hash: ValueHash,
    /// `true` if this is an input (source) node with no incoming edges.
    pub is_input: bool,
}

/// Graph-level metadata persisted under the sentinel key
/// [`StorageKey::graph_meta`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedGraphMeta {
    /// All edges in the graph (enough to reconstruct topology).
    pub edges: Vec<PersistedEdge>,
    /// Ordered list of all known node IDs (used to iterate during reload).
    pub node_ids: Vec<NodeId>,
}

// ---------------------------------------------------------------------------
// Encode / decode helpers
// ---------------------------------------------------------------------------

/// Encode a `Serialize`-able value to MessagePack bytes.
pub fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>, StorageError> {
    rmp_serde::to_vec(v).map_err(|e| StorageError::with_source("encode failed", e.to_string()))
}

/// Decode a `Deserialize`-able value from MessagePack bytes.
pub fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, StorageError> {
    rmp_serde::from_slice(bytes).map_err(|e| StorageError::with_source("decode failed", e.to_string()))
}

// ---------------------------------------------------------------------------
// In-memory Storage implementation (for testing)
// ---------------------------------------------------------------------------

/// A simple in-memory [`Storage`] implementation backed by a `tokio::sync::RwLock`.
///
/// **This is provided for tests and examples only.**  Do not use it in
/// production where data durability is required.
pub struct MemoryStorage {
    map: tokio::sync::RwLock<std::collections::HashMap<String, Vec<u8>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self { map: tokio::sync::RwLock::new(std::collections::HashMap::new()) }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn get(&self, key: &StorageKey) -> Result<Option<StorageValue>, StorageError> {
        let map = self.map.read().await;
        Ok(map.get(key.as_str()).map(|v| StorageValue::new(v.clone())))
    }

    async fn set(&self, key: &StorageKey, value: StorageValue) -> Result<(), StorageError> {
        let mut map = self.map.write().await;
        map.insert(key.0.clone(), value.into_bytes());
        Ok(())
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        let mut map = self.map.write().await;
        map.remove(key.as_str());
        Ok(())
    }

    async fn contains(&self, key: &StorageKey) -> Result<bool, StorageError> {
        let map = self.map.read().await;
        Ok(map.contains_key(key.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_storage_set_get_delete() {
        let s = MemoryStorage::new();
        let key = StorageKey::new("test");
        let val = StorageValue::new(vec![1, 2, 3]);

        assert!(!s.contains(&key).await.unwrap());
        s.set(&key, val.clone()).await.unwrap();
        assert!(s.contains(&key).await.unwrap());
        assert_eq!(s.get(&key).await.unwrap(), Some(val));
        s.delete(&key).await.unwrap();
        assert!(s.get(&key).await.unwrap().is_none());
    }

    #[test]
    fn encode_decode_round_trip() {
        let meta = PersistedGraphMeta { edges: vec![], node_ids: vec![] };
        let bytes = encode(&meta).unwrap();
        let back: PersistedGraphMeta = decode(&bytes).unwrap();
        assert_eq!(back.edges.len(), 0);
        assert_eq!(back.node_ids.len(), 0);
    }

    #[test]
    fn storage_key_for_node_is_consistent() {
        let id = NodeId::new();
        let k1 = StorageKey::for_node(id);
        let k2 = StorageKey::for_node(id);
        assert_eq!(k1, k2);
    }
}
