//! `EngineBuilder` — static topology declaration + `Engine` — sealed runtime.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::graph::{Graph, Endpoint};
use crate::loader::Loader;
use crate::node_id::NodeId;
use crate::scheduler::{Scheduler, UpdateReport};
use crate::storage::{Storage, StorageError};
use crate::transform::{Transform, ErasedTransform, IncrementalValue, TransformRegistrar};
use crate::value::{ValueTypeRegistry, ValueTypeRegistryBuilder, hash_value};

// ---------------------------------------------------------------------------
// EngineError (public)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EngineError {
    pub message: String,
}

impl EngineError {
    pub fn new(msg: impl Into<String>) -> Self { Self { message: msg.into() } }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for EngineError {}

impl From<crate::graph::GraphError> for EngineError {
    fn from(e: crate::graph::GraphError) -> Self { Self::new(e.to_string()) }
}
impl From<StorageError> for EngineError {
    fn from(e: StorageError) -> Self { Self::new(e.to_string()) }
}

// ---------------------------------------------------------------------------
// Builder state types
// ---------------------------------------------------------------------------

enum PendingNode {
    Input     { uuid: Uuid },
    Output    { uuid: Uuid },
    Transform { uuid: Uuid, key: String },
}

enum PendingEdge {
    IoToIo     { from: Uuid, to: Uuid },
    IoToSlot   { from: Uuid, to: Uuid,   slot: usize },
    SlotToIo   { from: Uuid, slot: usize, to: Uuid  },
    SlotToSlot { from: Uuid, out_slot: usize, to: Uuid, in_slot: usize },
}

/// A deferred transform constructor: takes a `ValueTypeRegistryBuilder`, registers
/// the transform's types into it, and returns a fully built [`ErasedTransform`].
type PendingTransform = Box<dyn FnOnce(&mut ValueTypeRegistryBuilder) -> ErasedTransform + Send>;

// ---------------------------------------------------------------------------
// EngineBuilder (public)
// ---------------------------------------------------------------------------

pub struct EngineBuilder {
    nodes:                Vec<PendingNode>,
    edges:                Vec<PendingEdge>,
    /// Deferred transform constructors — resolved during `build()`.
    pending_transforms:   HashMap<String, PendingTransform>,
    /// Maps transform UUID → registered key, for schema lookup during edge creation.
    uuid_to_key:          HashMap<Uuid, String>,
    cycle_limit:          u32,
    /// Accumulated errors from `register()` calls — surfaced by `build()`.
    registration_errors:  Vec<String>,
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self {
            nodes:               vec![],
            edges:               vec![],
            pending_transforms:  HashMap::new(),
            uuid_to_key:         HashMap::new(),
            cycle_limit:         1000,
            registration_errors: vec![],
        }
    }

    /// Register a transform type under `key`.
    ///
    /// Calls `T::register` to declare slot types.  Type registration and
    /// dispatch table construction happen during [`build`](Self::build).
    pub fn register<T: Transform>(mut self, key: &str, instance: T) -> Self {
        if self.pending_transforms.contains_key(key) {
            self.registration_errors.push(format!(
                "duplicate transform key {:?}", key
            ));
            return self;
        }
        let mut registrar = TransformRegistrar::new();
        T::register(&mut registrar);

        let pending: PendingTransform = Box::new(move |builder: &mut ValueTypeRegistryBuilder| {
            let (layout, dispatch) = registrar.finish_into(builder);
            ErasedTransform::new(layout, dispatch, instance)
        });
        self.pending_transforms.insert(key.to_owned(), pending);
        self
    }

    pub fn input_node<T: IncrementalValue>(mut self, id: Uuid) -> Self {
        self.nodes.push(PendingNode::Input { uuid: id });
        self
    }

    pub fn output_node(mut self, id: Uuid) -> Self {
        self.nodes.push(PendingNode::Output { uuid: id });
        self
    }

    pub fn transform_node(mut self, id: Uuid, key: &str) -> Self {
        self.uuid_to_key.insert(id, key.to_owned());
        self.nodes.push(PendingNode::Transform { uuid: id, key: key.to_owned() });
        self
    }

    pub fn wire(mut self, from: Uuid, to: Uuid) -> Self {
        self.edges.push(PendingEdge::IoToIo { from, to });
        self
    }

    pub fn wire_into_slot(mut self, from: Uuid, to_transform: Uuid, slot: usize) -> Self {
        self.edges.push(PendingEdge::IoToSlot { from, to: to_transform, slot });
        self
    }

    pub fn wire_slot_to(mut self, from_transform: Uuid, slot: usize, to: Uuid) -> Self {
        self.edges.push(PendingEdge::SlotToIo { from: from_transform, slot, to });
        self
    }

    pub fn wire_slot_to_slot(
        mut self,
        from_transform: Uuid, out_slot: usize,
        to_transform:   Uuid, in_slot:  usize,
    ) -> Self {
        self.edges.push(PendingEdge::SlotToSlot {
            from: from_transform, out_slot,
            to:   to_transform,   in_slot,
        });
        self
    }

    pub fn cycle_limit(mut self, limit: u32) -> Self { self.cycle_limit = limit; self }

    pub async fn build(self, storage: Arc<dyn Storage>) -> Result<Engine, EngineError> {
        // Surface any errors accumulated during register() calls.
        if !self.registration_errors.is_empty() {
            return Err(EngineError::new(
                self.registration_errors.join("; ")
            ));
        }

        // Build phase: resolve all pending transforms — each one registers its
        // slot types into the builder and produces an ErasedTransform.
        let mut reg_builder = ValueTypeRegistryBuilder::new();
        let transforms: HashMap<String, ErasedTransform> = self.pending_transforms
            .into_iter()
            .map(|(key, make)| (key, make(&mut reg_builder)))
            .collect();
        let registry = Arc::new(reg_builder.freeze());
        let graph    = Arc::new(Graph::new());

        let mut uuid_to_node: HashMap<Uuid, NodeId> = HashMap::new();

        for node in &self.nodes {
            match node {
                PendingNode::Input { uuid } => {
                    let nid = NodeId::from_uuid(*uuid);
                    uuid_to_node.insert(*uuid, nid);
                    graph.add_input_node(nid)?;
                }
                PendingNode::Output { uuid } => {
                    let nid = NodeId::from_uuid(*uuid);
                    uuid_to_node.insert(*uuid, nid);
                    graph.add_output_node(nid)?;
                }
                PendingNode::Transform { uuid, key } => {
                    let nid = NodeId::from_uuid(*uuid);
                    uuid_to_node.insert(*uuid, nid);
                    let erased = transforms.get(key.as_str())
                        .ok_or_else(|| EngineError::new(format!(
                            "transform key {key:?} not registered (declare with builder.register(...))"
                        )))?
                        .clone();
                    graph.add_transform_node(nid, key.clone(), erased)?;
                }
            }
        }

        // Helper: resolve UUID → NodeId.
        let lookup = |u: &Uuid| -> Result<NodeId, EngineError> {
            uuid_to_node.get(u).copied()
                .ok_or_else(|| EngineError::new(format!("node {u} not declared")))
        };

        // Helper: is output slot `slot` of transform `uuid` a collection?
        let out_is_col = |uuid: &Uuid, slot: usize| -> bool {
            self.uuid_to_key.get(uuid)
                .and_then(|k| transforms.get(k.as_str()))
                .and_then(|e| e.schema.outputs.get(slot))
                .map(|s| s.is_col)
                .unwrap_or(false)
        };

        // Helper: is input slot `slot` of transform `uuid` a collection?
        let in_is_col = |uuid: &Uuid, slot: usize| -> bool {
            self.uuid_to_key.get(uuid)
                .and_then(|k| transforms.get(k.as_str()))
                .and_then(|e| e.schema.inputs.get(slot))
                .map(|s| s.is_col)
                .unwrap_or(false)
        };

        // Compute the set of "fan-out" transforms transitively:
        // A transform is in fan-out mode if ANY of its input edges carries a collection
        // BUT its schema declares that slot as Single (not as input_collection).
        // This is the Collection→Single crossing that causes per-element invocation.
        //
        // A transform that declares input_collection IS a gather transform and is NOT
        // a fan-out transform (it receives the full collection itself).
        //
        // Propagation: if transform A is fan-out (outputs per-element collections) and
        // transform B receives A's output on a Single slot, B is also fan-out.
        //
        // Algorithm: iterate edges until the fan-out set stabilises.
        let mut fanout_transforms: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        let mut changed = true;
        while changed {
            changed = false;
            for edge in &self.edges {
                if let PendingEdge::SlotToSlot { from, out_slot, to, in_slot } = edge {
                    // This edge is collection if:
                    // (a) the source transform's schema declares collection output, OR
                    // (b) the source is a fan-out transform (its outputs accumulate as collections).
                    let is_c = out_is_col(from, *out_slot) || fanout_transforms.contains(from);
                    if is_c {
                        // Does 'to' receive this collection on a Single slot?
                        // (i.e., in_is_col for this slot is false → fan-out)
                        // OR on an explicitly declared collection slot → gather (NOT fan-out).
                        if !in_is_col(to, *in_slot) && !fanout_transforms.contains(to) {
                            fanout_transforms.insert(*to);
                            changed = true;
                        }
                    }
                }
            }
        }

        for edge in &self.edges {
            match edge {
                PendingEdge::IoToIo { from, to } => {
                    let f = lookup(from)?;
                    let t = lookup(to)?;
                    graph.add_edge(Endpoint::Io(f), Endpoint::Io(t), false)?;
                }
                PendingEdge::IoToSlot { from, to, slot } => {
                    let f = lookup(from)?;
                    let t = lookup(to)?;
                    graph.add_edge(Endpoint::Io(f),
                        Endpoint::TransformInput { transform: t, slot: *slot },
                        in_is_col(to, *slot))?;
                }
                PendingEdge::SlotToIo { from, slot, to } => {
                    let f = lookup(from)?;
                    let t = lookup(to)?;
                    // If the source transform is fan-out, its output to an IoNode is also collection.
                    // (But gather transforms with declared output_collection are handled by out_is_col.)
                    let is_c = out_is_col(from, *slot) || fanout_transforms.contains(from);
                    graph.add_edge(
                        Endpoint::TransformOutput { transform: f, slot: *slot },
                        Endpoint::Io(t),
                        is_c)?;
                }
                PendingEdge::SlotToSlot { from, out_slot, to, in_slot } => {
                    let f = lookup(from)?;
                    let t = lookup(to)?;
                    // Collection if source declares collection output, source is fan-out,
                    // OR target explicitly declares collection input.
                    let is_c = out_is_col(from, *out_slot)
                        || fanout_transforms.contains(from)
                        || in_is_col(to, *in_slot);
                    graph.add_edge(
                        Endpoint::TransformOutput { transform: f, slot: *out_slot },
                        Endpoint::TransformInput  { transform: t, slot: *in_slot  },
                        is_c)?;
                }
            }
        }

        let loader = Arc::new(Loader::new(Arc::clone(&storage), Arc::clone(&registry)));
        let mut sched = Scheduler::new(
            Arc::clone(&graph), Arc::clone(&loader), Arc::clone(&registry),
        );
        sched.set_cycle_limit(self.cycle_limit);

        // Warm start: restore cached values.
        // - Input nodes: use preload_input (no dirty marking) so set_input can hash-compare.
        // - Output/intermediate nodes: use store_output_on_node.
        let input_uuids: std::collections::HashSet<Uuid> = self.nodes.iter()
            .filter_map(|n| if let PendingNode::Input { uuid } = n { Some(*uuid) } else { None })
            .collect();

        for (uuid, nid) in &uuid_to_node {
            if let Ok(Some((v, h))) = loader.get(*nid).await {
                if input_uuids.contains(uuid) {
                    graph.preload_input(*nid, v, h);
                } else {
                    graph.store_output_on_node(*nid, v, h);
                }
            }
        }

        // On warm start, if we restored any cached values, mark all transforms Clean.
        // They will be re-dirtied by set_input() if any input hash changed.
        graph.mark_all_clean_if_warmed();

        Ok(Engine {
            graph,
            loader,
            scheduler: Arc::new(Mutex::new(sched)),
            storage,
            registry,
            checkpoint_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }
}

impl Default for EngineBuilder { fn default() -> Self { Self::new() } }

// ---------------------------------------------------------------------------
// Engine (public)
// ---------------------------------------------------------------------------

pub struct Engine {
    graph:             Arc<Graph>,
    loader:            Arc<Loader>,
    scheduler:         Arc<Mutex<Scheduler>>,
    storage:           Arc<dyn Storage>,
    registry:          Arc<ValueTypeRegistry>,
    checkpoint_active: Arc<std::sync::atomic::AtomicBool>,
}

impl Engine {
    /// Update the value of an input node.  No-op if hash is unchanged.
    pub fn set_input<T: IncrementalValue>(&self, id: Uuid, value: T) -> Result<(), EngineError> {
        let nid = NodeId::from_uuid(id);
        let v = self.registry.make_value(value)
            .map_err(|e| EngineError::new(e.message))?;
        let h = hash_value(&v, &self.registry);
        // Cache the input value so warm start can restore it next session.
        self.loader.cache(nid, v.clone(), h);
        self.graph.set_input(nid, v, h);
        Ok(())
    }

    /// Run one incremental update cycle.
    pub async fn update(&self) -> UpdateReport {
        self.scheduler.lock().await.run_update().await
    }

    /// Read the current value of a node.
    pub async fn get<T: IncrementalValue>(&self, id: Uuid) -> Result<Option<T>, EngineError> {
        let nid = NodeId::from_uuid(id);
        // Live output node value.
        if let Some((v, _)) = self.graph.peek_output(nid) {
            return self.registry.downcast_value::<T>(&v)
                .map(Some)
                .map_err(|e| EngineError::new(e.message));
        }
        // Storage cache.
        match self.loader.get(nid).await {
            Ok(Some((v, _))) => self.registry.downcast_value::<T>(&v)
                .map(Some).map_err(|e| EngineError::new(e.message)),
            Ok(None) => Ok(None),
            Err(e)   => Err(EngineError::from(e)),
        }
    }

    /// Begin a checkpoint.
    pub async fn checkpoint(&self) -> Result<(), EngineError> {
        if self.checkpoint_active.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(EngineError::new("checkpoint already active"));
        }
        self.storage.checkpoint().await.map_err(EngineError::from)
    }

    /// Commit all writes since `checkpoint()`.
    pub async fn commit(&self) -> Result<(), EngineError> {
        if !self.checkpoint_active.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(EngineError::new("no active checkpoint to commit"));
        }
        self.loader.flush_all().await.map_err(EngineError::from)?;
        self.storage.commit().await.map_err(EngineError::from)
    }

    /// Discard all writes since `checkpoint()`.
    pub async fn discard(&self) -> Result<(), EngineError> {
        if !self.checkpoint_active.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(EngineError::new("no active checkpoint to discard"));
        }
        // Evict the in-memory loader cache so get() falls through to storage.
        self.loader.evict_all();
        // Clear in-memory graph edge values so peek_output() returns None,
        // forcing get() to consult storage (which is now rolled back).
        self.graph.clear_io_values();
        self.storage.discard().await.map_err(EngineError::from)
    }
}
