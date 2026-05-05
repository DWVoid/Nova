//! [`EngineBuilder`] — static topology declaration + [`Engine`] — orchestration.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use uuid::Uuid;

use crate::keys::{NodeId, NodeInstanceKey, SubgraphId, UNIT_INSTANCE, workstate_storage_key};
use crate::report::UpdateReport;
use crate::runner::{RunContext, seed_pending_tasks, set_input_value};
use crate::storage::{Storage, StorageError, StorageValue};
use crate::task_queue::{SequentialTaskQueue, TaskQueue};
use crate::topology::{TopologyBuilder, NodeKind};
use crate::transform::{Transform, IncrementalValue, serialize_erased, hash_bytes, ErasedValue};
use crate::value_store::ValueStore;
use crate::workstate::{WorkState, WorkStateSnapshot, ValueHash};

// ---------------------------------------------------------------------------
// PendingInput — deferred input for async processing
// ---------------------------------------------------------------------------

/// A pending input value waiting to be processed during `update()`.
struct PendingInput {
    node_id: NodeId,
    erased: ErasedValue,
    bytes: Vec<u8>,
    hash: ValueHash,
    type_name: &'static str,
}

// ---------------------------------------------------------------------------
// EngineError
// ---------------------------------------------------------------------------

/// Error returned by engine construction or operation.
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

impl From<StorageError> for EngineError {
    fn from(e: StorageError) -> Self { Self::new(e.to_string()) }
}

// ---------------------------------------------------------------------------
// EngineBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for constructing an [`Engine`].
///
/// ## Required configuration
/// - [`with_storage`](EngineBuilder::with_storage) — must be called before [`build`](EngineBuilder::build).
///
/// ## Optional configuration
/// - [`with_task_queue`](EngineBuilder::with_task_queue) — supply a custom [`TaskQueue`] (default: [`SequentialTaskQueue`]).
/// - [`cycle_limit`](EngineBuilder::cycle_limit) — maximum execution cycles per node per update (default: 1000).
///
/// ## Example
/// ```rust,ignore
/// let engine = EngineBuilder::new()
///     .register("lex", LexTransform)
///     .input_node::<SourceFile>(file_node_id)
///     .transform_node(lex_node_id, "lex")
///     .wire_into_slot(file_node_id, lex_node_id, 0)
///     .with_storage(Arc::new(MemoryStorage::new()))
///     .build()
///     .await?;
/// ```
pub struct EngineBuilder {
    topo:        TopologyBuilder,
    cycle_limit: u32,
    storage:     Option<Arc<dyn Storage>>,
    task_queue:  Option<Box<dyn TaskQueue>>,
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self {
            topo:        TopologyBuilder::new(),
            cycle_limit: 1000,
            storage:     None,
            task_queue:  None,
        }
    }

    /// Register a transform instance under `key`.
    pub fn register<T: Transform>(mut self, key: &str, instance: T) -> Self {
        self.topo.register_transform(key, instance);
        self
    }

    /// Declare an I/O input node with the given UUID.
    pub fn input_node<T: IncrementalValue>(mut self, id: Uuid) -> Self {
        self.topo.add_io_input(id);
        self
    }

    /// Declare an I/O output node with the given UUID.
    pub fn output_node(mut self, id: Uuid) -> Self {
        self.topo.add_io_output(id);
        self
    }

    /// Declare a transform node, binding `id` to a registered transform `key`.
    pub fn transform_node(mut self, id: Uuid, key: &str) -> Self {
        self.topo.add_transform_node(id, key);
        self
    }

    /// Wire an I/O node's output (slot 0) directly to another I/O node's input (slot 0).
    pub fn wire(mut self, from: Uuid, to: Uuid) -> Self {
        self.topo.add_edge(from, 0, to, 0);
        self
    }

    /// Wire an I/O node's output into a specific input slot of a transform.
    pub fn wire_into_slot(mut self, from: Uuid, to_transform: Uuid, slot: usize) -> Self {
        self.topo.add_edge(from, 0, to_transform, slot);
        self
    }

    /// Wire a specific output slot of a transform to an I/O node's input (slot 0).
    pub fn wire_slot_to(mut self, from_transform: Uuid, slot: usize, to: Uuid) -> Self {
        self.topo.add_edge(from_transform, slot, to, 0);
        self
    }

    /// Wire an output slot of one transform into an input slot of another.
    pub fn wire_slot_to_slot(
        mut self,
        from_transform: Uuid, out_slot: usize,
        to_transform: Uuid, in_slot: usize,
    ) -> Self {
        self.topo.add_edge(from_transform, out_slot, to_transform, in_slot);
        self
    }

    /// Set the cycle limit (default: 1000).
    pub fn cycle_limit(mut self, limit: u32) -> Self {
        self.cycle_limit = limit;
        self.topo.set_cycle_limit(limit);
        self
    }

    /// Provide the storage backend (required before [`build`](Self::build)).
    pub fn with_storage(mut self, storage: Arc<dyn Storage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Provide a custom task queue.
    ///
    /// If not called, defaults to [`SequentialTaskQueue`].
    pub fn with_task_queue(mut self, queue: impl TaskQueue + 'static) -> Self {
        self.task_queue = Some(Box::new(queue));
        self
    }

    /// Validate and construct the [`Engine`].
    ///
    /// Requires [`with_storage`](Self::with_storage) to have been called.
    pub async fn build(self) -> Result<Engine, EngineError> {
        let storage = self.storage
            .ok_or_else(|| EngineError::new("storage is required — call .with_storage(...)"))?;
        let task_queue: Box<dyn TaskQueue> = self.task_queue
            .unwrap_or_else(|| Box::new(SequentialTaskQueue::new()));

        let topology = self.topo.freeze()
            .map_err(|errs| EngineError::new(errs.join("; ")))?;
        let topology = Arc::new(topology);
        let workstate = Arc::new(WorkState::new());
        let value_store = Arc::new(ValueStore::new());

        // Attempt warm start.
        let warmed = try_warm_start(&storage, &workstate, &topology).await;
        if !warmed {
            workstate.init_cold(&topology);
        }

        Ok(Engine {
            topology,
            workstate,
            value_store,
            task_queue: Arc::new(task_queue),
            storage,
            cycle_limit: self.cycle_limit,
            checkpoint_active: Arc::new(AtomicBool::new(false)),
            pending_inputs: Mutex::new(Vec::new()),
        })
    }
}

impl Default for EngineBuilder {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------------
// Warm start helper
// ---------------------------------------------------------------------------

async fn try_warm_start(
    storage: &Arc<dyn Storage>,
    workstate: &Arc<WorkState>,
    topology: &Arc<crate::topology::Topology>,
) -> bool {
    let key = workstate_storage_key();
    let bytes = match storage.get(&key).await {
        Ok(Some(sv)) => sv.as_bytes().to_vec(),
        _ => return false,
    };
    let snapshot: WorkStateSnapshot = match rmp_serde::from_slice(&bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    workstate.restore(snapshot, topology).await;
    true
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// The incremental computation engine.
pub struct Engine {
    topology:          Arc<crate::topology::Topology>,
    workstate:         Arc<WorkState>,
    value_store:       Arc<ValueStore>,
    /// Caller-provided (or default) task queue, wrapped in Arc for cloning into RunContext.
    task_queue:        Arc<Box<dyn TaskQueue>>,
    storage:           Arc<dyn Storage>,
    cycle_limit:       u32,
    checkpoint_active: Arc<AtomicBool>,
    /// Pending inputs accumulated by `set_input`, drained by `update`.
    pending_inputs:    Mutex<Vec<PendingInput>>,
}

impl Engine {
    /// Push a new value into an I/O input node identified by `id`.
    ///
    /// This method is synchronous and does not require a Tokio runtime context.
    /// The input is accumulated and will be processed during the next `update()` call.
    pub fn set_input<T: IncrementalValue>(&self, id: Uuid, value: T) -> Result<(), EngineError> {
        let node_id = NodeId::from_uuid(id);
        let _ = self.topology.node(node_id); // validate existence
        let bytes = serialize_erased(&value)
            .map_err(|e| EngineError::new(format!("serialize: {e}")))?;
        let hash = hash_bytes(&bytes);
        let erased = Arc::new(value) as crate::transform::ErasedValue;
        let type_name = std::any::type_name::<T>();

        // Accumulate the input for async processing during update().
        // This avoids the need for block_in_place and removes the Tokio runtime requirement.
        let mut pending = self.pending_inputs.lock().unwrap();
        pending.push(PendingInput {
            node_id,
            erased,
            bytes,
            hash,
            type_name,
        });
        Ok(())
    }

    /// Run all pending transforms until the graph converges.
    ///
    /// First resets per-cycle state (dirty flags, execution counts),
    /// then processes any pending inputs accumulated by `set_input()`,
    /// then runs all pending transforms.
    pub async fn update(&self) -> UpdateReport {
        let ctx = self.make_run_context();

        // Reset per-cycle state: clear dirty flags and execution counts.
        // This ensures only inputs that actually changed this cycle are marked dirty.
        ctx.workstate.reset_cycle();

        // Drain and process pending inputs.
        let pending: Vec<PendingInput> = {
            let mut guard = self.pending_inputs.lock().unwrap();
            std::mem::take(&mut *guard)
        };
        for input in pending {
            set_input_value(input.node_id, input.erased, input.bytes, input.hash, input.type_name, &ctx).await;
        }
        
        seed_pending_tasks(&ctx).await;
        ctx.task_queue.drain().await;
        let mutex = Arc::try_unwrap(ctx.report)
            .unwrap_or_else(|_| std::sync::Mutex::new(UpdateReport::default()));
        mutex.into_inner().unwrap()
    }

    /// Read the current value for the node identified by `id`.
    /// For I/O output nodes, resolves backwards through the incoming edge
    /// to find the actual source slot. Loads from storage on cache miss.
    pub async fn get<T: IncrementalValue>(&self, id: Uuid) -> Result<Option<T>, EngineError> {
        let node_id = NodeId::from_uuid(id);
        let node_desc = self.topology.node(node_id);

        // Determine the actual source slot key. For output nodes, follow
        // the incoming edge backward to the transform's output slot.
        let source_key = match &node_desc.kind {
            NodeKind::IoOutput => {
                let edge_ids = self.topology.incoming_edges(node_id, 0);
                match edge_ids.first() {
                    Some(&edge_id) => {
                        let edge = self.topology.edge(edge_id);
                        let src_sg = self.topology.node(edge.from_node).subgraph;
                        NodeInstanceKey::new(src_sg, UNIT_INSTANCE, edge.from_node)
                    }
                    None => return Ok(None),
                }
            }
            _ => NodeInstanceKey::new(SubgraphId(0), UNIT_INSTANCE, node_id),
        };
        let slot_key = source_key.slot_key(0);

        // Check cache first.
        if let Some(v) = self.value_store.get::<T>(slot_key) {
            return Ok(Some((*v).clone()));
        }

        // Lazy load from storage.
        let deserialize = |bytes: &[u8]| -> Result<ErasedValue, String> {
            let v: T = rmp_serde::from_slice(bytes)
                .map_err(|e| format!("{e}"))?;
            Ok(Arc::new(v) as ErasedValue)
        };
        let type_name = std::any::type_name::<T>();
        match self.value_store.load_erased(slot_key, self.storage.as_ref(), deserialize, type_name).await {
            Ok(Some((erased, _))) => {
                let typed = erased.as_any()
                    .downcast_ref::<T>()
                    .ok_or_else(|| EngineError::new("type mismatch on storage load"))?
                    .clone();
                Ok(Some(typed))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(EngineError::new(format!("storage load: {e}"))),
        }
    }

    /// Begin a storage checkpoint.
    pub async fn checkpoint(&self) -> Result<(), EngineError> {
        if self.checkpoint_active.swap(true, Ordering::SeqCst) {
            return Err(EngineError::new("checkpoint already active"));
        }
        self.storage.checkpoint().await.map_err(EngineError::from)
    }

    /// Commit all changes since the last checkpoint to storage.
    pub async fn commit(&self) -> Result<(), EngineError> {
        if !self.checkpoint_active.swap(false, Ordering::SeqCst) {
            return Err(EngineError::new("no active checkpoint to commit"));
        }
        self.value_store.flush(self.storage.as_ref()).await.map_err(EngineError::from)?;
        let snapshot = self.workstate.snapshot().await;
        let bytes = rmp_serde::to_vec(&snapshot)
            .map_err(|e| EngineError::new(format!("serialize workstate: {e}")))?;
        self.storage.set(&workstate_storage_key(), StorageValue::new(bytes))
            .await.map_err(EngineError::from)?;
        self.storage.commit().await.map_err(EngineError::from)
    }

    /// Discard all uncommitted changes and revert to the last committed state.
    pub async fn discard(&self) -> Result<(), EngineError> {
        if !self.checkpoint_active.swap(false, Ordering::SeqCst) {
            return Err(EngineError::new("no active checkpoint to discard"));
        }
        self.storage.discard().await.map_err(EngineError::from)?;
        self.value_store.evict_all();
        let warmed = try_warm_start(&self.storage, &self.workstate, &self.topology).await;
        if !warmed {
            self.workstate.init_cold(&self.topology);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn make_run_context(&self) -> RunContext {
        RunContext {
            topology:    Arc::clone(&self.topology),
            workstate:   Arc::clone(&self.workstate),
            value_store: Arc::clone(&self.value_store),
            task_queue:  Arc::clone(&self.task_queue),
            cycle_limit: self.cycle_limit,
            report:      Arc::new(std::sync::Mutex::new(UpdateReport::default())),
        }
    }
}

// ---------------------------------------------------------------------------
// Static Send/Sync assertions
// ---------------------------------------------------------------------------

fn _assert_engine_send() {
    fn is_send<T: Send>() {}
    is_send::<Engine>();
}
