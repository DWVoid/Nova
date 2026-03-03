//! `EngineBuilder` — static topology declaration + `Engine` — sealed runtime.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::execution_context::ExecutionContext;
use crate::loader::Loader;
use crate::node_id::NodeId;
use crate::scheduler::{Scheduler, UpdateReport};
use crate::storage::{Storage, StorageError};
use crate::topology::{Topology, TopologyBuilder, TopologyError, Endpoint};
use crate::transform::{Transform, ErasedTransform, IncrementalValue, TransformRegistrar};
use crate::value::{ValueTypeRegistryBuilder, hash_value};

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

impl From<TopologyError> for EngineError {
    fn from(e: TopologyError) -> Self { Self::new(e.to_string()) }
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
    nodes:               Vec<PendingNode>,
    edges:               Vec<PendingEdge>,
    pending_transforms:  HashMap<String, PendingTransform>,
    uuid_to_key:         HashMap<Uuid, String>,
    cycle_limit:         u32,
    registration_errors: Vec<String>,
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

    pub fn register<T: Transform>(mut self, key: &str, instance: T) -> Self {
        if self.pending_transforms.contains_key(key) {
            self.registration_errors.push(format!("duplicate transform key {:?}", key));
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
        if !self.registration_errors.is_empty() {
            return Err(EngineError::new(self.registration_errors.join("; ")));
        }

        // Resolve pending transforms: register types + produce ErasedTransform.
        let mut reg_builder = ValueTypeRegistryBuilder::new();
        let transforms: HashMap<String, ErasedTransform> = self.pending_transforms
            .into_iter()
            .map(|(key, make)| (key, make(&mut reg_builder)))
            .collect();
        let registry = Arc::new(reg_builder.freeze());

        // Build the topology.
        let mut topo_builder = TopologyBuilder::new();
        let mut uuid_to_node: HashMap<Uuid, NodeId> = HashMap::new();

        for node in &self.nodes {
            match node {
                PendingNode::Input { uuid } => {
                    let nid = NodeId::from_uuid(*uuid);
                    uuid_to_node.insert(*uuid, nid);
                    topo_builder.add_io_node(nid)?;
                }
                PendingNode::Output { uuid } => {
                    let nid = NodeId::from_uuid(*uuid);
                    uuid_to_node.insert(*uuid, nid);
                    topo_builder.add_io_node(nid)?;
                }
                PendingNode::Transform { uuid, key } => {
                    let nid = NodeId::from_uuid(*uuid);
                    uuid_to_node.insert(*uuid, nid);
                    let erased = transforms.get(key.as_str())
                        .ok_or_else(|| EngineError::new(format!(
                            "transform key {key:?} not registered"
                        )))?
                        .clone();
                    topo_builder.add_transform_node(nid, erased)?;
                }
            }
        }

        let lookup = |u: &Uuid| -> Result<NodeId, EngineError> {
            uuid_to_node.get(u).copied()
                .ok_or_else(|| EngineError::new(format!("node {u} not declared")))
        };

        let out_is_col = |uuid: &Uuid, slot: usize| -> bool {
            self.uuid_to_key.get(uuid)
                .and_then(|k| transforms.get(k.as_str()))
                .and_then(|e| e.schema.outputs.get(slot))
                .map(|s| s.is_col)
                .unwrap_or(false)
        };
        let in_is_col = |uuid: &Uuid, slot: usize| -> bool {
            self.uuid_to_key.get(uuid)
                .and_then(|k| transforms.get(k.as_str()))
                .and_then(|e| e.schema.inputs.get(slot))
                .map(|s| s.is_col)
                .unwrap_or(false)
        };

        // Compute fan-out set (Collection→Single crossing propagation).
        let mut fanout: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        let mut changed = true;
        while changed {
            changed = false;
            for edge in &self.edges {
                if let PendingEdge::SlotToSlot { from, out_slot, to, in_slot } = edge {
                    let is_c = out_is_col(from, *out_slot) || fanout.contains(from);
                    if is_c && !in_is_col(to, *in_slot) && fanout.insert(*to) {
                        changed = true;
                    }
                }
            }
        }

        for edge in &self.edges {
            match edge {
                PendingEdge::IoToIo { from, to } => {
                    topo_builder.add_edge(Endpoint::Io(lookup(from)?), Endpoint::Io(lookup(to)?), false)?;
                }
                PendingEdge::IoToSlot { from, to, slot } => {
                    topo_builder.add_edge(
                        Endpoint::Io(lookup(from)?),
                        Endpoint::TransformInput { transform: lookup(to)?, slot: *slot },
                        in_is_col(to, *slot),
                    )?;
                }
                PendingEdge::SlotToIo { from, slot, to } => {
                    let is_c = out_is_col(from, *slot) || fanout.contains(from);
                    topo_builder.add_edge(
                        Endpoint::TransformOutput { transform: lookup(from)?, slot: *slot },
                        Endpoint::Io(lookup(to)?),
                        is_c,
                    )?;
                }
                PendingEdge::SlotToSlot { from, out_slot, to, in_slot } => {
                    let is_c = out_is_col(from, *out_slot) || fanout.contains(from) || in_is_col(to, *in_slot);
                    topo_builder.add_edge(
                        Endpoint::TransformOutput { transform: lookup(from)?, slot: *out_slot },
                        Endpoint::TransformInput  { transform: lookup(to)?,   slot: *in_slot  },
                        is_c,
                    )?;
                }
            }
        }

        let topology = Arc::new(topo_builder.freeze(Arc::clone(&registry)));

        let loader  = Arc::new(Loader::new(Arc::clone(&storage), Arc::clone(&registry)));
        let mut exec = ExecutionContext::new(Arc::clone(&loader), Arc::clone(&registry));

        // Initialise WorkState edge values.
        for (eid, edge) in &topology.edges {
            exec.init_edge(*eid, edge.is_collection);
        }

        // Mark all transform nodes dirty initially.
        exec.workstate.mark_all_dirty(topology.transform_nodes.keys());

        // Warm start: restore cached values.
        let input_uuids: std::collections::HashSet<Uuid> = self.nodes.iter()
            .filter_map(|n| if let PendingNode::Input { uuid } = n { Some(*uuid) } else { None })
            .collect();

        for (uuid, &nid) in &uuid_to_node {
            if let Ok(Some((v, h))) = loader.get(nid).await {
                if input_uuids.contains(uuid) {
                    exec.preload_input_value(&topology, nid, v, h);
                } else {
                    // Write output value onto the incoming edge of the IoNode.
                    if let Some(adj) = topology.io_adjacency(nid) {
                        if let Some(in_eid) = adj.incoming {
                            exec.workstate.preload_single(in_eid, v, h);
                        }
                    }
                }
            }
        }

        // On warm start: mark all transforms clean if we restored any output.
        exec.try_mark_all_clean_on_warm_start(&topology);

        let mut sched = Scheduler::new();
        sched.cycle_limit = self.cycle_limit;

        Ok(Engine {
            topology,
            exec: Arc::new(Mutex::new(exec)),
            storage,
            sched: Arc::new(sched),
            checkpoint_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }
}

impl Default for EngineBuilder { fn default() -> Self { Self::new() } }

// ---------------------------------------------------------------------------
// Engine (public)
// ---------------------------------------------------------------------------

pub struct Engine {
    topology:          Arc<Topology>,
    exec:              Arc<Mutex<ExecutionContext>>,
    storage:           Arc<dyn Storage>,
    sched:             Arc<Scheduler>,
    checkpoint_active: Arc<std::sync::atomic::AtomicBool>,
}

impl Engine {
    /// Update the value of an input node.  No-op if hash is unchanged.
    pub fn set_input<T: IncrementalValue>(&self, id: Uuid, value: T) -> Result<(), EngineError> {
        let nid = NodeId::from_uuid(id);
        let registry = &self.topology.registry;
        let v = registry.make_value(value).map_err(|e| EngineError::new(e.message))?;
        let h = hash_value(&v, registry);
        // Cache for warm start (synchronous write to in-memory cache).
        // Note: exec lock is needed to update WorkState.
        // We use try_lock since set_input is sync; callers shouldn't call during update.
        if let Ok(mut exec) = self.exec.try_lock() {
            exec.loader.cache(nid, v.clone(), h);
            exec.set_input_value(&self.topology, nid, v, h);
        }
        Ok(())
    }

    /// Run one incremental update cycle.
    pub async fn update(&self) -> UpdateReport {
        let cycle_limit = self.sched.cycle_limit;
        let mut report  = UpdateReport::default();
        let mut iteration = 0u32;

        loop {
            let task_graph = {
                let mut exec = self.exec.lock().await;
                exec.propagate_removals(&self.topology);
                exec.build_task_graph(&self.topology)
            };

            if task_graph.is_empty() { break; }

            if iteration >= cycle_limit {
                // Collect the remaining dirty node ids as cycle-limit violators.
                let exec = self.exec.lock().await;
                for &id in self.topology.topo_order() {
                    if exec.is_dirty(id) {
                        report.cycle_limit_exceeded.push(id.as_uuid());
                    }
                }
                break;
            }

            let n_tasks = task_graph.len();
            let results = self.sched.run(task_graph).await;

            let (changed, coll_changed, errors) = {
                let mut exec = self.exec.lock().await;
                exec.apply_results(&self.topology, results).await
            };

            report.transforms_evaluated        += n_tasks;
            report.transforms_changed          += changed;
            report.collection_elements_changed += coll_changed;
            report.errors.extend(errors);
            iteration += 1;
        }

        report
    }

    /// Read the current value of a node.
    pub async fn get<T: IncrementalValue>(&self, id: Uuid) -> Result<Option<T>, EngineError> {
        let nid      = NodeId::from_uuid(id);
        let registry = &self.topology.registry;
        {
            let exec = self.exec.lock().await;
            if let Some((v, _)) = exec.peek_output(&self.topology, nid) {
                return registry.downcast_value::<T>(&v)
                    .map(Some)
                    .map_err(|e| EngineError::new(e.message));
            }
        }
        // Fall back to storage.
        let exec = self.exec.lock().await;
        match exec.loader.get(nid).await {
            Ok(Some((v, _))) => registry.downcast_value::<T>(&v)
                .map(Some).map_err(|e| EngineError::new(e.message)),
            Ok(None) => Ok(None),
            Err(e)   => Err(EngineError::from(e)),
        }
    }

    pub async fn checkpoint(&self) -> Result<(), EngineError> {
        if self.checkpoint_active.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(EngineError::new("checkpoint already active"));
        }
        self.storage.checkpoint().await.map_err(EngineError::from)
    }

    pub async fn commit(&self) -> Result<(), EngineError> {
        if !self.checkpoint_active.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(EngineError::new("no active checkpoint to commit"));
        }
        let exec = self.exec.lock().await;
        exec.loader.flush_all().await.map_err(EngineError::from)?;
        self.storage.commit().await.map_err(EngineError::from)
    }

    pub async fn discard(&self) -> Result<(), EngineError> {
        if !self.checkpoint_active.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(EngineError::new("no active checkpoint to discard"));
        }
        {
            let mut exec = self.exec.lock().await;
            exec.loader.evict_all();
            exec.clear_values_and_mark_all_dirty(&self.topology);
        }
        self.storage.discard().await.map_err(EngineError::from)
    }
}
