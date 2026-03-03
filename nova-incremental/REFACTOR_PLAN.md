# nova-incremental Refactor Plan
## Date: 2026-03-03

### Goals
1. Delete `execution_context.rs` and `graph.rs` (God-object anti-patterns / dead code)
2. Scheduler is zero-dependency, general-purpose, incremental dispatch
3. WorkState owns all domain logic + topology-aware algorithms
4. Warm start via serialized StateSnapshot (no more heuristics)
5. Mark-and-sweep storage eviction

---

### Module Responsibilities

#### scheduler.rs (ZERO crate deps)
- `pub struct TaskId(u64)` — opaque handle
- `pub struct Scheduler` — Arc<SchedulerInner> internally; dispatcher loop via tokio::spawn + mpsc
- `pub fn spawn(f: Future) -> TaskId` — fire and forget, immediately eligible
- `pub fn schedule(deps: &[TaskId], f: Future) -> TaskId` — runs when all deps done
- `pub async fn join()` — drains all tasks; invalidates all TaskIds
- `pub struct UpdateReport` — stays here

Internals:
- `SchedulerInner { counter: AtomicU64, waiting: Mutex<Vec<WaitingTask>>, done: Mutex<HashSet<u64>>, active: AtomicUsize, notify: Notify }`
- WaitingTask { id, deps: Vec<u64>, f: BoxFuture }
- On task complete: remove from active, insert into done, notify dispatcher
- Dispatcher (tokio::spawn in Scheduler::new): loop { notify.notified(); scan waiting → dispatch satisfied }
- join(): wait until active==0 && waiting is empty

#### workstate.rs
- `WorkState { inner: Arc<RwLock<WorkStateInner>> }`
- `WorkStateInner { node_status: HashMap<NodeId, NodeStatus>, edge_values: HashMap<EdgeId, EdgeValue> }`
- All existing mutation methods on `WorkStateInner` (write_single, upsert_element, etc.)
- New WorkState methods (take &Topology):
  - `init_from_topology(topo)` — init edges + mark all dirty
  - `set_input(topo, node, value, hash) -> bool`
  - `preload_input(topo, node, value, hash)`
  - `peek_output(topo, node) -> Option<(Value, ValueHash)>`
  - `propagate_removals(topo)` — pre-pass before run_pass
  - `async run_pass(topo, loader, registry, sched) -> PassResult`
  - `snapshot() -> StateSnapshot`
  - `restore(topo, snapshot: StateSnapshot)`

run_pass algorithm:
  1. Acquire read lock; collect dirty NodeIds in topo order
  2. Create Arc<SegQueue<TaskOutput>> as output sink
  3. Build NodeId→TaskId map
  4. For each dirty transform:
     a. Read inputs (acquiring read lock briefly)
     b. Clone erased transform + inputs into self-contained TransformPayload
     c. Compute deps (other dirty NodeIds feeding this one → their TaskIds)
     d. sched.spawn() or sched.schedule(deps, async move { payload.run().await; sink.push(output) })
     e. Store NodeId→TaskId
  5. sched.join().await
  6. Drain sink; for each TaskOutput:
     a. Acquire write lock; apply to edge_values; dirty downstream; mark clean/error
  7. Return PassResult { changed, coll_changed, errors }

#### engine.rs (thin)
- `Engine { topology: Arc<Topology>, workstate: WorkState, loader: Arc<Loader>, storage: Arc<dyn Storage>, scheduler: Scheduler, checkpoint_active: AtomicBool }`
- `set_input<T>` → `workstate.set_input(topology, ...)`
- `update()` loop → `ws.propagate_removals(topo); ws.run_pass(topo, loader, registry, sched).await; repeat until nothing changed`
- `checkpoint/commit` → `ws.snapshot()` → serialize → `loader.flush_all()` → `storage.set(STATE_KEY, bytes)` → `storage.commit()`
- `discard` → reload snapshot from storage → `ws.restore(topo, snapshot)` → `loader.evict_all()`
- `get<T>` → `workstate.peek_output(topology, id)` → fallback to loader

#### StateSnapshot (in workstate.rs, Serialize/Deserialize)
```rust
struct StateSnapshot {
    node_status: HashMap<NodeId, bool>,  // true=dirty
    edge_values: HashMap<EdgeId, SnapshotEdge>,
}
enum SnapshotEdge {
    Single { type_key: String, value_bytes: Vec<u8>, hash: u64, dirty: bool },
    Collection(Vec<SnapshotCollectionElement>),
}
struct SnapshotCollectionElement { key: u64, type_key: String, value_bytes: Vec<u8>, hash: u64, dirty: bool }
```
Storage key: `StorageKey::state()` using `const STATE_UUID: Uuid = ...`

#### Mark-and-sweep in Loader
- `begin_tracking()` — snapshots set of currently stored keys
- `record_write(key)` — called on each persist
- `sweep(active_nodes: &[NodeId])` — deletes stored keys not reachable from active_nodes
- Called: `begin_tracking` at start of `update()`, `sweep` during `commit()`

#### Deleted
- `execution_context.rs` — entirely removed
- `graph.rs` — entirely removed
- Tests in `tests.rs` rewritten (all graph.rs tests → Engine public API tests)

---

### File Change List
- MODIFY: scheduler.rs (complete rewrite — zero deps, incremental dispatch)
- MODIFY: workstate.rs (add topology-aware methods, run_pass, snapshot/restore)
- MODIFY: engine.rs (simplify, remove ExecutionContext usage)
- MODIFY: loader.rs (add begin_tracking / record_write / sweep)
- MODIFY: storage.rs (add StorageKey::state() constant)
- MODIFY: lib.rs (remove execution_context module)
- MODIFY: tests.rs (rewrite graph.rs tests, add snapshot test)
- DELETE: execution_context.rs
- DELETE: graph.rs
