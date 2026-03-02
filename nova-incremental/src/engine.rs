//! Top-level [`IncrementalEngine`] facade.
//!
//! ## New API (bipartite graph)
//!
//! The engine now exposes explicit node and edge construction:
//!
//! ```no_run
//! use nova_incremental::{IncrementalEngine, storage::MemoryStorage};
//! use nova_incremental::graph::Endpoint;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut engine = IncrementalEngine::new(Arc::new(MemoryStorage::new()));
//!
//!     let input  = engine.add_input_node::<i32>(21).unwrap();
//!     let output = engine.add_output_node();
//!     let t = engine.add_transform_node("double").unwrap();
//!
//!     engine.connect_single_input(input, t, 0).unwrap();
//!     engine.connect_single_output(t, 0, output).unwrap();
//!
//!     engine.update().await;
//!     let v: i32 = engine.get_value(output).await.unwrap().unwrap();
//!     assert_eq!(v, 42);
//! }
//! ```
//!
//! ## Backward Compatibility
//!
//! `register_one_to_one` / `register_many_to_one` / `register_one_to_many` /
//! `register_many_to_many` and the old `connect(&[sources], &[targets], key)`
//! shim are still available for existing code (e.g. `SemanticSession`).

use std::any::{Any, TypeId};
use std::cmp::Ordering;
use std::future::Future;
use std::sync::Arc;
use serde::{Serialize, de::DeserializeOwned};

use crate::graph::{EdgeId, Endpoint, Graph};
use crate::loader::LazyLoader;
use crate::node_id::NodeId;
use crate::registry::{SorterFn, TransformRegistry};
use crate::scheduler::{Scheduler, UpdateReport};
use crate::storage::{Storage, StorageError};
use crate::transform::{
    Transform, TransformError, TransformFn,
    TypedOneToOne, TypedManyToOne, TypedOneToMany, TypedManyToMany,
};
use crate::value::{Value, ValueTypeRegistry, RegistryError, hash_bytes};

// ---------------------------------------------------------------------------
// EngineError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum EngineError {
    Graph(crate::graph::GraphError),
    Storage(StorageError),
    UnknownTransform(String),
    UnknownValueType(String),
    UnknownSorter(String),
    TypeMismatch { expected: String, actual: String },
    UnregisteredType(String),
    Other(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Graph(e)         => write!(f, "graph error: {e}"),
            EngineError::Storage(e)       => write!(f, "storage error: {e}"),
            EngineError::UnknownTransform(k) => write!(f, "unknown transform key: {k}"),
            EngineError::UnknownValueType(k) => write!(f, "unknown value type key: {k}"),
            EngineError::UnknownSorter(k)   => write!(f, "unknown sorter key: {k}"),
            EngineError::TypeMismatch { expected, actual } =>
                write!(f, "type mismatch: expected {expected:?}, got {actual:?}"),
            EngineError::UnregisteredType(t) => write!(f, "unregistered type: {t}"),
            EngineError::Other(s)         => write!(f, "{s}"),
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

pub struct IncrementalEngine {
    graph:          Arc<Graph>,
    loader:         Arc<LazyLoader>,
    scheduler:      Scheduler,
    transforms:     TransformRegistry,
    sorters:        Arc<std::sync::RwLock<std::collections::HashMap<String, SorterFn>>>,
    value_registry: Arc<ValueTypeRegistry>,
    storage:        Arc<dyn Storage>,
}

impl IncrementalEngine {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        let vr = ValueTypeRegistry::new();
        vr.register_primitives().expect("primitive registration must not fail");
        let value_registry = Arc::new(vr);
        let graph   = Arc::new(Graph::new());
        let loader  = Arc::new(LazyLoader::new(Arc::clone(&storage), Arc::clone(&value_registry)));
        let sorters = Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
        let mut scheduler = Scheduler::new(Arc::clone(&graph), Arc::clone(&loader));
        scheduler.set_sorters(Arc::clone(&sorters));
        Self {
            graph, loader, scheduler,
            transforms: TransformRegistry::new(),
            sorters,
            value_registry,
            storage,
        }
    }

    // -----------------------------------------------------------------------
    // Type registration
    // -----------------------------------------------------------------------

    pub fn register_value_type<T>(&mut self, key: &str) -> Result<(), EngineError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        self.value_registry.register::<T>(key).map_err(EngineError::from)
    }

    // -----------------------------------------------------------------------
    // Sorter registration
    // -----------------------------------------------------------------------

    /// Register a comparator function under `key` for use in `Collection` slots.
    pub fn register_sorter(
        &mut self,
        key: &str,
        cmp: impl Fn(&Value, &Value) -> Ordering + Send + Sync + 'static,
    ) {
        self.sorters.write().unwrap().insert(key.to_string(), Arc::new(cmp));
    }

    // -----------------------------------------------------------------------
    // Cycle limit
    // -----------------------------------------------------------------------

    pub fn set_cycle_limit(&mut self, limit: u32) {
        self.scheduler.set_cycle_limit(limit);
    }

    // -----------------------------------------------------------------------
    // Transform registration
    // -----------------------------------------------------------------------

    /// Register an arbitrary transform with a full slot schema.
    pub fn register_transform(
        &mut self,
        key: &str,
        f: Arc<dyn TransformFn>,
    ) -> Result<(), EngineError> {
        // Validate that all type keys in the schema are registered.
        let schema = f.schema().clone();
        for slot in schema.inputs.iter().chain(schema.outputs.iter()) {
            if !self.value_registry.contains_key(&slot.type_key) {
                return Err(EngineError::UnknownValueType(slot.type_key.clone()));
            }
        }
        self.transforms.register(key, Transform::new(f));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Backward-compat transform registration shims
    // -----------------------------------------------------------------------

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
        self.transforms.register(key, Transform::new(Arc::new(adapter)));
        Ok(())
    }

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
        self.transforms.register(key, Transform::new(Arc::new(adapter)));
        Ok(())
    }

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
        self.transforms.register(key, Transform::new(Arc::new(adapter)));
        Ok(())
    }

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
        self.transforms.register(key, Transform::new(Arc::new(adapter)));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Node construction (new explicit API)
    // -----------------------------------------------------------------------

    /// Add an input node with an initial typed value.
    pub fn add_input_node<T>(&self, value: T) -> Result<NodeId, EngineError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let v = self.value_registry.make_value(value)?;
        let h = hash_bytes(&self.value_registry.serialize_value(&v));
        let id = self.graph.add_input_node();
        self.loader.cache_value(id, v.clone());
        self.graph.set_input(id, v, h).expect("node was just created");
        Ok(id)
    }

    /// Add an output node (computed by an upstream transform).
    pub fn add_output_node(&self) -> NodeId {
        self.graph.add_output_node()
    }

    /// Add a transform node by registered key.
    ///
    /// The transform's slot schema (number of inputs/outputs, kinds) is
    /// determined by the registered transform.
    pub fn add_transform_node(&self, key: &str) -> Result<NodeId, EngineError> {
        let t = self.transforms.get(key)
            .ok_or_else(|| EngineError::UnknownTransform(key.to_string()))?
            .clone();
        Ok(self.graph.add_transform_node(key, t))
    }

    // -----------------------------------------------------------------------
    // Slot wiring (new explicit API)
    // -----------------------------------------------------------------------

    /// Wire `from_node` (InputNode or OutputNode) to input slot `slot` of `transform`.
    pub fn connect_single_input(&self, from_node: NodeId, transform: NodeId, slot: usize) -> Result<EdgeId, EngineError> {
        Ok(self.graph.add_single_edge(
            Endpoint::Io(from_node),
            Endpoint::TransformInput { transform, slot },
        )?)
    }

    /// Wire output slot `slot` of `transform` to `to_node` (OutputNode).
    pub fn connect_single_output(&self, transform: NodeId, slot: usize, to_node: NodeId) -> Result<EdgeId, EngineError> {
        Ok(self.graph.add_single_edge(
            Endpoint::TransformOutput { transform, slot },
            Endpoint::Io(to_node),
        )?)
    }

    /// Wire `from_node` to collection input slot `slot` of `transform`.
    pub fn connect_collection_input(&self, from_node: NodeId, transform: NodeId, slot: usize) -> Result<EdgeId, EngineError> {
        Ok(self.graph.add_collection_edge(
            Endpoint::Io(from_node),
            Endpoint::TransformInput { transform, slot },
        )?)
    }

    /// Wire collection output slot `slot` of `transform` to `to_node`.
    pub fn connect_collection_output(&self, transform: NodeId, slot: usize, to_node: NodeId) -> Result<EdgeId, EngineError> {
        Ok(self.graph.add_collection_edge(
            Endpoint::TransformOutput { transform, slot },
            Endpoint::Io(to_node),
        )?)
    }

    /// Wire output slot of one transform directly to input slot of another
    /// (transform-to-transform, Single).
    pub fn connect_transform_to_transform(
        &self,
        from_transform: NodeId, out_slot: usize,
        to_transform:   NodeId, in_slot:  usize,
    ) -> Result<EdgeId, EngineError> {
        Ok(self.graph.add_single_edge(
            Endpoint::TransformOutput { transform: from_transform, slot: out_slot },
            Endpoint::TransformInput  { transform: to_transform,   slot: in_slot  },
        )?)
    }

    /// Wire output slot of one transform directly to collection input slot of another.
    pub fn connect_transform_to_collection(
        &self,
        from_transform: NodeId, out_slot: usize,
        to_transform:   NodeId, in_slot:  usize,
    ) -> Result<EdgeId, EngineError> {
        Ok(self.graph.add_collection_edge(
            Endpoint::TransformOutput { transform: from_transform, slot: out_slot },
            Endpoint::TransformInput  { transform: to_transform,   slot: in_slot  },
        )?)
    }

    // -----------------------------------------------------------------------
    // Backward-compat: connect + add_input / add_output shims
    // -----------------------------------------------------------------------

    /// Old-style: add an input node, same as `add_input_node`.
    pub fn add_input<T>(&self, value: T) -> Result<NodeId, EngineError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        self.add_input_node(value)
    }

    /// Connect `sources` to `targets` via the named transform (compat shim).
    ///
    /// Creates one `TransformNode` internally.  For multi-input transforms,
    /// all sources are wired to successive input slots; all targets to
    /// successive output slots.
    pub fn connect(
        &self,
        sources: &[NodeId],
        targets: &[NodeId],
        transform_key: &str,
    ) -> Result<(), EngineError> {
        let t = self.transforms.get(transform_key)
            .ok_or_else(|| EngineError::UnknownTransform(transform_key.to_string()))?
            .clone();
        let tid = self.graph.add_transform_node(transform_key, t.clone());

        let n_in  = t.schema().inputs.len();
        let n_out = t.schema().outputs.len();

        // If schema has 0 input slots (dynamic-count shim like ManyToOne),
        // extend the transform node's input_edges to accommodate all sources.
        if n_in == 0 && !sources.is_empty() {
            if let Some(mut tn) = self.graph.get_transform_node_mut(tid) {
                tn.input_edges = vec![vec![]; sources.len()];
            }
        }
        // Same for output slots.
        if n_out == 0 && !targets.is_empty() {
            if let Some(mut tn) = self.graph.get_transform_node_mut(tid) {
                tn.output_edges = vec![vec![]; targets.len()];
            }
        }

        for (slot, &src) in sources.iter().enumerate() {
            let actual_slot = if n_in > 0 { slot.min(n_in - 1) } else { slot };
            self.graph.add_single_edge(Endpoint::Io(src), Endpoint::TransformInput { transform: tid, slot: actual_slot })?;
        }
        for (slot, &tgt) in targets.iter().enumerate() {
            let actual_slot = if n_out > 0 { slot.min(n_out - 1) } else { slot };
            self.graph.add_single_edge(Endpoint::TransformOutput { transform: tid, slot: actual_slot }, Endpoint::Io(tgt))?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Input updates
    // -----------------------------------------------------------------------

    pub fn set_input<T>(&self, id: NodeId, value: T) -> Result<(), EngineError>
    where
        T: Any + Send + Sync + Clone + Serialize + DeserializeOwned + 'static,
    {
        let v = self.value_registry.make_value(value)?;
        let h = hash_bytes(&self.value_registry.serialize_value(&v));
        self.loader.cache_value(id, v.clone());
        self.graph.set_input(id, v, h)?;
        Ok(())
    }

    pub fn remove_node(&self, id: NodeId) -> bool {
        self.loader.evict(id);
        self.graph.remove_io_node(id) || self.graph.remove_transform_node(id)
    }

    // -----------------------------------------------------------------------
    // Update cycle
    // -----------------------------------------------------------------------

    pub async fn update(&self) -> UpdateReport {
        self.scheduler.run_update().await
    }

    // -----------------------------------------------------------------------
    // Persistence (stubs — full re-implementation pending)
    // -----------------------------------------------------------------------

    /// Persist the engine state to the backing storage.
    ///
    /// **Not yet fully implemented for the bipartite graph model.**
    /// Currently only persists the in-memory loader cache via the storage
    /// backend's flush mechanism.
    pub async fn save(&self) -> Result<(), EngineError> {
        // TODO: serialize graph topology and all edge values to storage.
        // For now, flush whatever the loader has cached.
        self.loader.flush_all().await.map_err(EngineError::Storage)
    }

    /// Restore an engine from storage into `proto` (which must have all
    /// transforms registered).
    ///
    /// **Not yet fully implemented for the bipartite graph model.**
    /// Currently only restores node IDs found in storage; graph topology
    /// must be rebuilt by the caller.
    pub async fn load(storage: Arc<dyn Storage>, proto: IncrementalEngine) -> Result<Self, EngineError> {
        // TODO: deserialize graph topology from storage and reconnect edges.
        // For now, return proto as-is (transforms and type registrations are preserved).
        Ok(proto)
    }

    // -----------------------------------------------------------------------
    // Value access
    // -----------------------------------------------------------------------

    pub async fn get_value<T>(&self, id: NodeId) -> Result<Option<T>, EngineError>
    where
        T: Any + Clone + Serialize + DeserializeOwned + 'static,
    {
        // Prefer edge-cached value on the incoming edge of an OutputNode.
        let v_opt: Option<Value> = if let Some((v, _)) = self.graph.peek_io_value(id) {
            Some(v)
        } else {
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
    // Graph accessors
    // -----------------------------------------------------------------------

    pub fn graph(&self) -> Arc<Graph> { Arc::clone(&self.graph) }
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
        let b = engine.graph.add_output_node();
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
    async fn explicit_node_wiring() {
        let engine = make_engine();
        let input  = engine.add_input_node::<i32>(7).unwrap();
        let output = engine.add_output_node();
        let t      = engine.add_transform_node("double").unwrap();
        engine.connect_single_input(input, t, 0).unwrap();
        engine.connect_single_output(t, 0, output).unwrap();
        let report = engine.update().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let v: i32 = engine.get_value(output).await.unwrap().unwrap();
        assert_eq!(v, 14);
    }
}
