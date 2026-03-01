//! Top-level [`IncrementalEngine`] facade.
//!
//! ## Design: Single Entry Point
//!
//! Users of the incremental system only need to interact with
//! `IncrementalEngine`.  All other types (`Graph`, `Scheduler`, `LazyLoader`,
//! `TransformRegistry`) are created and wired together internally.
//!
//! This façade pattern:
//! - Reduces the API surface users need to understand.
//! - Enforces invariants (e.g. transforms must be registered before edges are
//!   added that reference them).
//! - Makes it easy to swap internal implementations without breaking callers.
//!
//! ## Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use nova_incremental::{
//!     IncrementalEngine,
//!     value::Value,
//!     transform::{Transform, OneToOneTransform, TransformError},
//!     storage::MemoryStorage,
//!     registry::TransformRegistry,
//! };
//! use async_trait::async_trait;
//!
//! struct Double;
//! #[async_trait]
//! impl OneToOneTransform for Double {
//!     async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
//!         let n = *input.downcast::<i32>().unwrap();
//!         Ok(Value::new(n * 2))
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let storage = Arc::new(MemoryStorage::new());
//!     let mut engine = IncrementalEngine::new(storage);
//!     engine.register_transform("double", Transform::OneToOne(Arc::new(Double)));
//!
//!     let input = engine.add_input(Value::new(21i32));
//!     let output = engine.add_output_node();
//!     engine.connect(&[input], &[output], "double").unwrap();
//!
//!     let report = engine.update().await;
//!     let v = engine.get_value(output).await.unwrap().unwrap();
//!     assert_eq!(v.downcast::<i32>(), Some(&42i32));
//! }
//! ```
use std::sync::Arc;
use crate::graph::{Graph, NodeEntry};
use crate::loader::LazyLoader;
use crate::node_id::NodeId;
use crate::registry::TransformRegistry;
use crate::scheduler::{Scheduler, UpdateReport};
use crate::storage::{
    Storage, StorageKey, StorageValue, StorageError,
    PersistedGraphMeta, PersistedEdge, encode, decode,
};
use crate::transform::Transform;
use crate::value::{Value, hash_bytes};
/// Error returned by [`IncrementalEngine`] operations.
#[derive(Debug)]
pub enum EngineError {
    Graph(crate::graph::GraphError),
    Storage(StorageError),
    UnknownTransform(String),
    Other(String),
}
impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Graph(e) => write!(f, "graph error: {e}"),
            EngineError::Storage(e) => write!(f, "storage error: {e}"),
            EngineError::UnknownTransform(k) => write!(f, "unknown transform key: {k}"),
            EngineError::Other(s) => write!(f, "{s}"),
        }
    }
}
impl std::error::Error for EngineError {}
impl From<crate::graph::GraphError> for EngineError {
    fn from(e: crate::graph::GraphError) -> Self { EngineError::Graph(e) }
}
impl From<StorageError> for EngineError {
    fn from(e: StorageError) -> Self { EngineError::Storage(e) }
}
/// The top-level incremental computation engine.
///
/// Create one with [`IncrementalEngine::new`], register transforms, build the
/// graph, feed inputs, and call [`update`](Self::update) whenever inputs
/// change.
pub struct IncrementalEngine {
    graph: Arc<Graph>,
    loader: Arc<LazyLoader>,
    scheduler: Scheduler,
    registry: TransformRegistry,
    storage: Arc<dyn Storage>,
}
impl IncrementalEngine {
    /// Create a new engine backed by the given storage.
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        let graph = Arc::new(Graph::new());
        let loader = Arc::new(LazyLoader::new(Arc::clone(&storage)));
        let scheduler = Scheduler::new(Arc::clone(&graph), Arc::clone(&loader));
        Self {
            graph,
            loader,
            scheduler,
            registry: TransformRegistry::new(),
            storage,
        }
    }
    // -----------------------------------------------------------------------
    // Transform registration
    // -----------------------------------------------------------------------
    /// Register a transform under a stable string key.
    ///
    /// The key must match the `transform_key` used when calling [`connect`](Self::connect)
    /// and must be re-registered in the same way when restoring a persisted
    /// graph.
    pub fn register_transform(&mut self, key: impl Into<String>, transform: Transform) {
        self.registry.register(key, transform);
    }
    // -----------------------------------------------------------------------
    // Graph construction
    // -----------------------------------------------------------------------
    /// Add an input node with an initial value and return its ID.
    ///
    /// The node is immediately marked dirty so the first call to
    /// [`update`](Self::update) will propagate its value.
    pub fn add_input(&self, value: Value) -> NodeId {
        let id = self.graph.add_input_node();
        self.graph.set_input(id, value).expect("node was just created");
        id
    }
    /// Add a computed output node (no initial value; computed by a transform).
    pub fn add_output_node(&self) -> NodeId {
        self.graph.add_computed_node()
    }
    /// Connect `sources` to `targets` via the transform registered under
    /// `transform_key`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::UnknownTransform`] if `transform_key` is not
    /// registered.  Returns [`EngineError::Graph`] if any node ID is unknown.
    pub fn connect(
        &self,
        sources: &[NodeId],
        targets: &[NodeId],
        transform_key: &str,
    ) -> Result<(), EngineError> {
        let transform = self.registry.get(transform_key)
            .ok_or_else(|| EngineError::UnknownTransform(transform_key.to_string()))?
            .clone();
        self.graph.add_transform(
            sources.to_vec(),
            targets.to_vec(),
            transform,
            transform_key,
        )?;
        Ok(())
    }

    /// Alias for [`connect`](Self::connect).
    ///
    /// Provided so call sites that deal with pre-assigned node IDs can use a
    /// more explicit name, making it clear that no new IDs are allocated.
    pub fn connect_by_key(
        &self,
        sources: &[NodeId],
        targets: &[NodeId],
        transform_key: &str,
    ) -> Result<(), EngineError> {
        self.connect(sources, targets, transform_key)
    }
    // -----------------------------------------------------------------------
    // Input updates
    // -----------------------------------------------------------------------
    /// Update the value of an input node and mark it (and all dependents) dirty.
    ///
    /// Call [`update`](Self::update) afterwards to propagate the change.
    pub fn set_input(&self, id: NodeId, value: Value) -> Result<(), EngineError> {
        // Keep loader cache in sync.
        self.loader.cache_value(id, value.clone());
        self.graph.set_input(id, value)?;
        Ok(())
    }

    /// Remove a node (and all edges that touch it) from the graph.
    ///
    /// Any nodes that previously depended on `id` are marked dirty so the
    /// next [`update`](Self::update) call will attempt to recompute them
    /// (which will fail unless the missing input is replaced or those
    /// downstream nodes are also removed).
    ///
    /// The value is also evicted from the loader's in-memory cache.
    ///
    /// Returns `true` if the node existed, `false` if it was already absent.
    pub fn remove_node(&self, id: NodeId) -> bool {
        self.loader.evict(id);
        self.graph.remove_node(id)
    }
    // -----------------------------------------------------------------------
    // Update cycle
    // -----------------------------------------------------------------------
    /// Run one incremental update cycle, recomputing all dirty nodes in
    /// parallel waves.
    ///
    /// Returns an [`UpdateReport`] with statistics and any errors.
    pub async fn update(&self) -> UpdateReport {
        self.scheduler.run_update().await
    }
    // -----------------------------------------------------------------------
    // Value access
    // -----------------------------------------------------------------------
    /// Lazily load the current value of `id`.
    ///
    /// Returns `Ok(None)` if the node has never been computed or its value
    /// was evicted.
    pub async fn get_value(&self, id: NodeId) -> Result<Option<Value>, EngineError> {
        // Check in-graph cache first (fastest path).
        if let Some((v, _)) = self.graph.peek_value(id) {
            return Ok(Some(v));
        }
        // Fall back to loader (checks memory cache, then storage).
        Ok(self.loader.get(id).await?)
    }
    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------
    /// Persist the full graph topology and all in-memory node values to
    /// storage.
    ///
    /// Node values that are not in the in-memory cache are skipped (they are
    /// already in storage from a previous persist call).
    pub async fn save(&self) -> Result<(), EngineError> {
        // Persist graph metadata (topology).
        let edges: Vec<PersistedEdge> = self.graph.all_edges().iter().map(|e| PersistedEdge {
            sources: e.sources.clone(),
            targets: e.targets.clone(),
            transform_key: e.transform_key.clone(),
        }).collect();
        let node_ids = self.graph.all_node_ids();
        let meta = PersistedGraphMeta { edges, node_ids: node_ids.clone() };
        let meta_bytes = encode(&meta)?;
        self.storage.set(&StorageKey::graph_meta(), StorageValue::new(meta_bytes)).await?;
        // Persist each node that has an in-graph value.
        //
        // Value::to_bytes() uses the serde closure captured at Value::new time,
        // so the stored bytes are always a valid MessagePack encoding – never
        // a raw memory representation.  The hash is computed from those same
        // bytes so it is value-level (two logically-equal values → same hash).
        for nid in node_ids {
            if let Some((value, _old_hash)) = self.graph.peek_value(nid) {
                let is_input = self.graph.is_input(nid);
                let bytes = value.to_bytes();
                let hash = hash_bytes(&bytes);
                // Update the graph's stored hash to the byte-based one.
                let _ = self.graph.store_value(nid, value.clone(), hash);
                self.loader.persist(nid, value, hash, is_input).await?;
            }
        }
        Ok(())
    }
    /// Restore an engine from storage, re-associating transforms from
    /// `registry`.
    ///
    /// Any transform key found in storage that is absent from `registry` will
    /// cause an error.
    pub async fn load(
        storage: Arc<dyn Storage>,
        registry: TransformRegistry,
    ) -> Result<Self, EngineError> {
        let meta_bytes = storage.get(&StorageKey::graph_meta()).await?
            .ok_or_else(|| EngineError::Other("no graph metadata in storage".to_string()))?;
        let meta: PersistedGraphMeta = decode(meta_bytes.as_bytes())?;
        let graph = Arc::new(Graph::new());
        let loader = Arc::new(LazyLoader::new(Arc::clone(&storage)));
        // Restore all nodes.
        for nid in &meta.node_ids {
            let node_data = loader.load_node_data(*nid).await?;
            let is_input = node_data.as_ref().map(|d| d.is_input).unwrap_or(false);
            let entry = if is_input {
                NodeEntry::new_input(*nid)
            } else {
                NodeEntry::new_computed(*nid)
            };
            graph.register_node(entry);
            // Restore last-known hash for change detection.
            if let Some(d) = node_data {
                if d.value_hash != 0 {
                    // Store a sentinel empty value with the known hash so
                    // hash-based early exit works after reload.
                    let _ = graph.store_value(*nid, Value::new(d.value_bytes), d.value_hash);
                }
            }
        }
        // Restore edges.
        for pe in &meta.edges {
            let transform = registry.get(&pe.transform_key)
                .ok_or_else(|| EngineError::UnknownTransform(pe.transform_key.clone()))?
                .clone();
            // Build an EdgeEntry directly (edge IDs are local-only).
            use crate::graph::EdgeEntry;
            let eid = {
                use std::sync::atomic::{AtomicU64, Ordering};
                static RELOAD_COUNTER: AtomicU64 = AtomicU64::new(1_000_000);
                crate::graph::EdgeId(RELOAD_COUNTER.fetch_add(1, Ordering::Relaxed))
            };
            let entry = EdgeEntry {
                id: eid,
                transform,
                sources: pe.sources.clone(),
                targets: pe.targets.clone(),
                transform_key: pe.transform_key.clone(),
            };
            graph.register_edge(entry);
            // Update adjacency lists manually (register_edge doesn't do it).
            for &s in &pe.sources {
                if let Some(mut n) = graph.nodes_mut(s) { n.outgoing.push(eid); }
            }
            for &t in &pe.targets {
                if let Some(mut n) = graph.nodes_mut(t) { n.incoming.push(eid); }
            }
        }
        let scheduler = Scheduler::new(Arc::clone(&graph), Arc::clone(&loader));
        Ok(Self { graph, loader, scheduler, registry, storage })
    }
    // -----------------------------------------------------------------------
    // Accessors (for testing / introspection)
    // -----------------------------------------------------------------------
    /// Return a clone of the `Arc<Graph>` (for advanced use / testing).
    pub fn graph(&self) -> Arc<Graph> { Arc::clone(&self.graph) }
    /// Return a clone of the `Arc<LazyLoader>`.
    pub fn loader(&self) -> Arc<LazyLoader> { Arc::clone(&self.loader) }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use crate::transform::{Transform, OneToOneTransform, TransformError};
    use async_trait::async_trait;
    use std::sync::Arc;
    struct Double;
    #[async_trait]
    impl OneToOneTransform for Double {
        async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
            let n = input.downcast::<i32>().copied().ok_or_else(|| TransformError::new("i32"))?;
            Ok(Value::new(n * 2))
        }
    }
    fn make_engine() -> IncrementalEngine {
        let storage = Arc::new(MemoryStorage::new());
        let mut engine = IncrementalEngine::new(storage);
        engine.register_transform("double", Transform::OneToOne(Arc::new(Double)));
        engine
    }
    #[tokio::test]
    async fn basic_one_to_one_pipeline() {
        let engine = make_engine();
        let input = engine.add_input(Value::new(21i32));
        let output = engine.add_output_node();
        engine.connect(&[input], &[output], "double").unwrap();
        let report = engine.update().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let v = engine.get_value(output).await.unwrap().unwrap();
        assert_eq!(v.downcast::<i32>(), Some(&42i32));
    }
    #[tokio::test]
    async fn unknown_transform_returns_error() {
        let engine = make_engine();
        let a = engine.graph.add_input_node();
        let b = engine.graph.add_computed_node();
        let result = engine.connect(&[a], &[b], "nonexistent");
        assert!(matches!(result, Err(EngineError::UnknownTransform(_))));
    }
    #[tokio::test]
    async fn set_input_propagates_dirty() {
        let engine = make_engine();
        let input = engine.add_input(Value::new(1i32));
        let output = engine.add_output_node();
        engine.connect(&[input], &[output], "double").unwrap();
        engine.update().await;
        // Change input.
        engine.set_input(input, Value::new(5i32)).unwrap();
        let report = engine.update().await;
        assert!(report.is_ok());
        let v = engine.get_value(output).await.unwrap().unwrap();
        assert_eq!(v.downcast::<i32>(), Some(&10i32));
    }
    #[tokio::test]
    async fn save_and_reload() {
        let storage = Arc::new(MemoryStorage::new());
        let mut engine = IncrementalEngine::new(Arc::clone(&storage) as Arc<dyn Storage>);
        engine.register_transform("double", Transform::OneToOne(Arc::new(Double)));
        let input = engine.add_input(Value::new(7i32));
        let output = engine.add_output_node();
        engine.connect(&[input], &[output], "double").unwrap();
        engine.update().await;
        engine.save().await.unwrap();
        // Reload.
        let mut registry = TransformRegistry::new();
        registry.register("double", Transform::OneToOne(Arc::new(Double)));
        let engine2 = IncrementalEngine::load(Arc::clone(&storage) as Arc<dyn Storage>, registry)
            .await.unwrap();
        // Graph topology should be restored.
        assert!(engine2.graph.contains_node(input));
        assert!(engine2.graph.contains_node(output));
    }
}
