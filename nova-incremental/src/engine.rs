//! Top-level [`IncrementalEngine`] facade – the sole public entry point.
//!
//! ## Public API
//!
//! All value types and transforms are accessed through typed methods only.
//! [`Value`] and [`Transform`] are `pub(crate)` implementation details.
//!
//! ## Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use nova_incremental::{IncrementalEngine, transform::TransformError, storage::MemoryStorage};
//!
//! #[tokio::main]
//! async fn main() {
//!     let storage = Arc::new(MemoryStorage::new());
//!     let mut engine = IncrementalEngine::new(storage);
//!     engine.register_value_type::<i32>("i32").unwrap();
//!     engine.register_one_to_one::<i32, i32, _, _>("double",
//!         |n: &i32| { let n = *n; async move { Ok(n * 2) } }).unwrap();
//!
//!     let input  = engine.add_input(21i32).unwrap();
//!     let output = engine.add_output_node();
//!     engine.connect(&[input], &[output], "double").unwrap();
//!
//!     engine.update().await;
//!     let v: i32 = engine.get_value(output).await.unwrap().unwrap();
//!     assert_eq!(v, 42);
//! }
//! ```

use std::any::{Any, TypeId};
use std::future::Future;
use std::sync::Arc;
use serde::{Serialize, de::DeserializeOwned};

use crate::graph::{Graph, NodeEntry};
use crate::loader::LazyLoader;
use crate::node_id::NodeId;
use crate::registry::TransformRegistry;
use crate::scheduler::{Scheduler, UpdateReport};
use crate::storage::{
    Storage, StorageKey, StorageValue, StorageError,
    PersistedGraphMeta, PersistedEdge, encode, decode,
};
use crate::transform::{
    Transform, TransformError,
    TypedOneToOne, TypedManyToOne, TypedOneToMany, TypedManyToMany,
};
use crate::value::{Value, ValueTypeRegistry, RegistryError, hash_bytes};

// ---------------------------------------------------------------------------
// EngineError
// ---------------------------------------------------------------------------

/// Error returned by [`IncrementalEngine`] operations.
#[derive(Debug)]
pub enum EngineError {
    Graph(crate::graph::GraphError),
    Storage(StorageError),
    UnknownTransform(String),
    UnknownValueType(String),
    TypeMismatch { expected: String, actual: String },
    UnregisteredType(String),
    Other(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Graph(e) => write!(f, "graph error: {e}"),
            EngineError::Storage(e) => write!(f, "storage error: {e}"),
            EngineError::UnknownTransform(k) => write!(f, "unknown transform key: {k}"),
            EngineError::UnknownValueType(k) => write!(f, "unknown value type key: {k}"),
            EngineError::TypeMismatch { expected, actual } =>
                write!(f, "type mismatch: expected {expected:?}, got {actual:?}"),
            EngineError::UnregisteredType(t) => write!(f, "unregistered type: {t}"),
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

impl From<RegistryError> for EngineError {
    fn from(e: RegistryError) -> Self { EngineError::UnregisteredType(e.message) }
}

// ---------------------------------------------------------------------------
// IncrementalEngine
// ---------------------------------------------------------------------------

/// The top-level incremental computation engine.
///
/// All value types and transform functions must be registered here before use.
/// Primitive types (`bool`, all integer widths, `f32`, `f64`, `String`) are
/// pre-registered automatically on construction.
pub struct IncrementalEngine {
    graph: Arc<Graph>,
    loader: Arc<LazyLoader>,
    scheduler: Scheduler,
    transforms: TransformRegistry,
    value_registry: Arc<ValueTypeRegistry>,
    storage: Arc<dyn Storage>,
}

impl IncrementalEngine {
    /// Create a new engine backed by the given storage.
    ///
    /// Primitive types are registered automatically.
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        let vr = ValueTypeRegistry::new();
        vr.register_primitives().expect("primitive registration must not fail");
        let value_registry = Arc::new(vr);
        let graph = Arc::new(Graph::new());
        let loader = Arc::new(LazyLoader::new(Arc::clone(&storage), Arc::clone(&value_registry)));
        let scheduler = Scheduler::new(Arc::clone(&graph), Arc::clone(&loader));
        Self { graph, loader, scheduler, transforms: TransformRegistry::new(), value_registry, storage }
    }

    // -----------------------------------------------------------------------
    // Type registration
    // -----------------------------------------------------------------------

    /// Register a value type `T` under a stable string `key`.
    ///
    /// - Idempotent if the same `(T, key)` pair is registered again.
    /// - Returns an error if `key` maps to a different type or vice-versa.
    /// - Primitives (`i32`, `u64`, `String`, …) are pre-registered; calling
    ///   this for them with the same key is a no-op.
    pub fn register_value_type<T>(&mut self, key: &str) -> Result<(), EngineError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        self.value_registry.register::<T>(key).map_err(EngineError::from)
    }

    // -----------------------------------------------------------------------
    // Transform registration (typed, closure-friendly)
    // -----------------------------------------------------------------------

    /// Register a 1→1 transform.  `In` and `Out` must already be registered
    /// as value types.
    pub fn register_one_to_one<In, Out, F, Fut>(
        &mut self,
        key: &str,
        f: F,
    ) -> Result<(), EngineError>
    where
        In:  Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
        Out: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
        F:   Fn(&In) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
    {
        self.check_type_registered::<In>()?;
        self.check_type_registered::<Out>()?;
        let adapter = TypedOneToOne::new(f, Arc::clone(&self.value_registry));
        self.transforms.register(key, Transform::OneToOne(Arc::new(adapter)));
        Ok(())
    }

    /// Register a N→1 transform.
    pub fn register_many_to_one<In, Out, F, Fut>(
        &mut self,
        key: &str,
        f: F,
    ) -> Result<(), EngineError>
    where
        In:  Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
        Out: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
        F:   Fn(&[In]) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Out, TransformError>> + Send + 'static,
    {
        self.check_type_registered::<In>()?;
        self.check_type_registered::<Out>()?;
        let adapter = TypedManyToOne::new(f, Arc::clone(&self.value_registry));
        self.transforms.register(key, Transform::ManyToOne(Arc::new(adapter)));
        Ok(())
    }

    /// Register a 1→N transform.
    pub fn register_one_to_many<In, Out, F, Fut>(
        &mut self,
        key: &str,
        f: F,
    ) -> Result<(), EngineError>
    where
        In:  Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
        Out: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
        F:   Fn(&In) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
    {
        self.check_type_registered::<In>()?;
        self.check_type_registered::<Out>()?;
        let adapter = TypedOneToMany::new(f, Arc::clone(&self.value_registry));
        self.transforms.register(key, Transform::OneToMany(Arc::new(adapter)));
        Ok(())
    }

    /// Register a N→M transform.
    pub fn register_many_to_many<In, Out, F, Fut>(
        &mut self,
        key: &str,
        f: F,
    ) -> Result<(), EngineError>
    where
        In:  Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
        Out: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
        F:   Fn(&[In]) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<Out>, TransformError>> + Send + 'static,
    {
        self.check_type_registered::<In>()?;
        self.check_type_registered::<Out>()?;
        let adapter = TypedManyToMany::new(f, Arc::clone(&self.value_registry));
        self.transforms.register(key, Transform::ManyToMany(Arc::new(adapter)));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Graph construction
    // -----------------------------------------------------------------------

    /// Add an input node with an initial typed value and return its ID.
    ///
    /// `T` must be registered. Fails with [`EngineError::UnregisteredType`]
    /// otherwise.
    pub fn add_input<T>(&self, value: T) -> Result<NodeId, EngineError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let v = self.value_registry.make_value(value)?;
        let id = self.graph.add_input_node();
        self.loader.cache_value(id, v.clone());
        self.graph.set_input(id, v).expect("node was just created");
        Ok(id)
    }

    /// Add a computed output node (no initial value; computed by a transform).
    pub fn add_output_node(&self) -> NodeId {
        self.graph.add_computed_node()
    }

    /// Connect `sources` to `targets` via the transform registered under
    /// `transform_key`.
    pub fn connect(
        &self,
        sources: &[NodeId],
        targets: &[NodeId],
        transform_key: &str,
    ) -> Result<(), EngineError> {
        let transform = self.transforms.get(transform_key)
            .ok_or_else(|| EngineError::UnknownTransform(transform_key.to_string()))?
            .clone();
        self.graph.add_transform(sources.to_vec(), targets.to_vec(), transform, transform_key)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Input updates
    // -----------------------------------------------------------------------

    /// Update the value of an input node and mark it (and all dependents)
    /// dirty.  `T` must be registered.
    pub fn set_input<T>(&self, id: NodeId, value: T) -> Result<(), EngineError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let v = self.value_registry.make_value(value)?;
        self.loader.cache_value(id, v.clone());
        self.graph.set_input(id, v)?;
        Ok(())
    }

    /// Remove a node (and all edges that touch it) from the graph.
    pub fn remove_node(&self, id: NodeId) -> bool {
        self.loader.evict(id);
        self.graph.remove_node(id)
    }

    // -----------------------------------------------------------------------
    // Update cycle
    // -----------------------------------------------------------------------

    /// Run one incremental update cycle, recomputing all dirty nodes.
    pub async fn update(&self) -> UpdateReport {
        self.scheduler.run_update().await
    }

    // -----------------------------------------------------------------------
    // Value access (typed, read-only)
    // -----------------------------------------------------------------------

    /// Lazily load and return the current value of `id` as `T`.
    ///
    /// - Returns `Ok(None)` if the node has never been computed.
    /// - Returns `Err(EngineError::TypeMismatch)` if the stored type key does
    ///   not match `T`.
    /// - Returns `Err(EngineError::Storage)` if a storage read fails.
    /// - **Does not mutate** graph or node status.
    pub async fn get_value<T>(&self, id: NodeId) -> Result<Option<T>, EngineError>
    where
        T: Any + Clone + Serialize + DeserializeOwned + 'static,
    {
        // Prefer in-graph cache (fastest, no await).
        let v_opt: Option<Value> = if let Some((v, _)) = self.graph.peek_value(id) {
            Some(v)
        } else {
            // Fall back to loader (in-memory cache then storage).
            self.loader.get(id).await.map_err(EngineError::Storage)?
        };

        match v_opt {
            None => Ok(None),
            Some(v) => {
                self.value_registry
                    .downcast_value::<T>(&v)
                    .map(Some)
                    .map_err(|_| EngineError::TypeMismatch {
                        expected: self.value_registry
                            .key_for_type_id(TypeId::of::<T>())
                            .unwrap_or_else(|| "<unregistered>".to_string()),
                        actual: self.value_registry.type_key_of(&v).to_string(),
                    })
            }
        }
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Persist full graph topology and all in-memory node values to storage.
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
        for nid in node_ids {
            if let Some((value, _)) = self.graph.peek_value(nid) {
                let is_input = self.graph.is_input(nid);
                let bytes = self.value_registry.serialize_value(&value);
                let hash = hash_bytes(&bytes);
                let _ = self.graph.store_value(nid, value.clone(), hash);
                self.loader.persist(nid, value, hash, is_input).await?;
            }
        }
        Ok(())
    }

    /// Restore an engine from storage.
    ///
    /// The caller must supply a pre-configured engine (with all value types and
    /// transforms registered) so the loader can reconstruct typed values from
    /// the persisted bytes.
    pub async fn load(
        storage: Arc<dyn Storage>,
        mut engine: IncrementalEngine,
    ) -> Result<Self, EngineError> {
        let meta_bytes = storage.get(&StorageKey::graph_meta()).await?
            .ok_or_else(|| EngineError::Other("no graph metadata in storage".to_string()))?;
        let meta: PersistedGraphMeta = decode(meta_bytes.as_bytes())?;

        let graph = Arc::new(Graph::new());
        let loader = Arc::new(LazyLoader::new(
            Arc::clone(&storage),
            Arc::clone(&engine.value_registry),
        ));

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

            // Restore typed value and last-known hash.
            if let Some(d) = node_data {
                if !d.value_bytes.is_empty() && d.value_hash != 0 {
                    // Validate the type key is registered before loading.
                    if !engine.value_registry.contains_key(&d.type_key) {
                        return Err(EngineError::UnknownValueType(d.type_key.clone()));
                    }
                    let value = engine.value_registry
                        .deserialize_value(&d.type_key, &d.value_bytes)
                        .map_err(|e| EngineError::Other(e.message))?;
                    let _ = graph.store_value(*nid, value.clone(), d.value_hash);
                    loader.cache_value(*nid, value);
                }
            }
        }

        // Restore edges.
        for pe in &meta.edges {
            let transform = engine.transforms.get(&pe.transform_key)
                .ok_or_else(|| EngineError::UnknownTransform(pe.transform_key.clone()))?
                .clone();
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
            for &s in &pe.sources {
                if let Some(mut n) = graph.nodes_mut(s) { n.outgoing.push(eid); }
            }
            for &t in &pe.targets {
                if let Some(mut n) = graph.nodes_mut(t) { n.incoming.push(eid); }
            }
        }

        let scheduler = Scheduler::new(Arc::clone(&graph), Arc::clone(&loader));
        Ok(Self {
            graph,
            loader,
            scheduler,
            transforms: engine.transforms,
            value_registry: engine.value_registry,
            storage,
        })
    }

    // -----------------------------------------------------------------------
    // Accessors (for testing / introspection)
    // -----------------------------------------------------------------------

    /// Return a clone of the `Arc<Graph>` (for advanced use / testing).
    pub fn graph(&self) -> Arc<Graph> { Arc::clone(&self.graph) }

    /// Return a clone of the `Arc<LazyLoader>`.
    pub fn loader(&self) -> Arc<LazyLoader> { Arc::clone(&self.loader) }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn check_type_registered<T: Any + 'static>(&self) -> Result<(), EngineError> {
        if self.value_registry.key_for_type_id(TypeId::of::<T>()).is_none() {
            return Err(EngineError::UnregisteredType(format!(
                "type `{}` is not registered; call register_value_type::<{0}>() first",
                std::any::type_name::<T>()
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use std::sync::Arc;

    fn make_engine() -> IncrementalEngine {
        let storage = Arc::new(MemoryStorage::new());
        let mut engine = IncrementalEngine::new(storage);
        engine.register_one_to_one::<i32, i32, _, _>("double",
            |n: &i32| { let n = *n; async move { Ok(n * 2) } }).unwrap();
        engine
    }

    #[tokio::test]
    async fn basic_one_to_one_pipeline() {
        let engine = make_engine();
        let input  = engine.add_input(21i32).unwrap();
        let output = engine.add_output_node();
        engine.connect(&[input], &[output], "double").unwrap();
        let report = engine.update().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let v: i32 = engine.get_value(output).await.unwrap().unwrap();
        assert_eq!(v, 42);
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
        let input  = engine.add_input(1i32).unwrap();
        let output = engine.add_output_node();
        engine.connect(&[input], &[output], "double").unwrap();
        engine.update().await;
        engine.set_input(input, 5i32).unwrap();
        let report = engine.update().await;
        assert!(report.is_ok());
        let v: i32 = engine.get_value(output).await.unwrap().unwrap();
        assert_eq!(v, 10);
    }

    #[tokio::test]
    async fn get_value_type_mismatch_returns_error() {
        let engine = make_engine();
        let input = engine.add_input(42i32).unwrap();
        // Try to read as u64 – should produce TypeMismatch.
        let result = engine.get_value::<u64>(input).await;
        assert!(matches!(result, Err(EngineError::TypeMismatch { .. })));
    }

    #[tokio::test]
    async fn unregistered_type_fails_on_add_input() {
        let engine = make_engine();
        #[derive(Clone, serde::Serialize, serde::Deserialize)]
        struct Custom(i32);
        let result = engine.add_input(Custom(1));
        assert!(matches!(result, Err(EngineError::UnregisteredType(_))));
    }

    #[tokio::test]
    async fn save_and_reload_typed_values() {
        let storage = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
        let mut engine = IncrementalEngine::new(Arc::clone(&storage));
        engine.register_one_to_one::<i32, i32, _, _>("double",
            |n: &i32| { let n = *n; async move { Ok(n * 2) } }).unwrap();

        let input  = engine.add_input(7i32).unwrap();
        let output = engine.add_output_node();
        engine.connect(&[input], &[output], "double").unwrap();
        engine.update().await;
        engine.save().await.unwrap();

        // Reload with a fresh engine (same transforms registered).
        let mut engine2 = IncrementalEngine::new(Arc::clone(&storage));
        engine2.register_one_to_one::<i32, i32, _, _>("double",
            |n: &i32| { let n = *n; async move { Ok(n * 2) } }).unwrap();
        let engine2 = IncrementalEngine::load(Arc::clone(&storage), engine2).await.unwrap();

        assert!(engine2.graph.contains_node(input));
        assert!(engine2.graph.contains_node(output));
        // Value should be fully typed after reload.
        let v: i32 = engine2.get_value(output).await.unwrap().unwrap();
        assert_eq!(v, 14);
    }
}
