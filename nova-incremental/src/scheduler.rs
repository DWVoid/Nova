//! Parallel recomputation engine.
//!
//! ## Design: Tokio Tasks vs. Rayon
//!
//! Transforms are defined as `async` functions (see `transform.rs`).  Rayon
//! operates on OS threads with a blocking interface; mixing Rayon and Tokio
//! is possible but adds complexity (blocking tasks must use
//! `tokio::task::spawn_blocking`).  We therefore use Tokio's work-stealing
//! scheduler throughout:
//!
//! - Each node recomputation is a `tokio::task::spawn` task.
//! - Tasks within the same "wave" (see below) run concurrently on the Tokio
//!   thread pool.
//! - CPU-bound transforms will naturally benefit from Tokio's multi-threaded
//!   runtime (`tokio::runtime::Builder::new_multi_thread`).
//!
//! If a host application uses transforms that are purely CPU-bound and
//! blocking, they should call `tokio::task::spawn_blocking` internally.
//!
//! ## Design: Wave-Based Parallel Execution
//!
//! The dirty subgraph is processed in topological waves:
//!
//! - **Wave 0**: dirty input nodes (no unresolved dirty predecessors).
//! - **Wave 1**: nodes whose only dirty predecessors are in Wave 0.
//! - **Wave K**: nodes whose dirty predecessors all belong to waves < K.
//!
//! All nodes within a wave are independent of each other and can run in
//! parallel.  After a wave completes we start the next wave, which may have
//! grown (because nodes in the previous wave may have been marked dirty
//! downstream – though with eager dirty propagation from `mark_dirty` this
//! is not needed here).
//!
//! ## Design: Hash-Based Early Exit
//!
//! After a transform runs, the scheduler hashes the outputs.  If all output
//! hashes match the previously stored hashes, no downstream nodes are marked
//! dirty and the persist step is skipped.  This can eliminate large swaths
//! of the recomputation graph when an input change ultimately has no effect
//! on a particular output (e.g. a whitespace change in source that the
//! parser normalises away).
use std::collections::HashMap;
use std::sync::Arc;
use futures::future::join_all;
use crate::graph::Graph;
use crate::loader::LazyLoader;
use crate::node_id::NodeId;
use crate::transform::TransformError;
use crate::value::{Value, hash_bytes};
// ---------------------------------------------------------------------------
// Update report
// ---------------------------------------------------------------------------
/// Summary of a single incremental update cycle.
#[derive(Debug, Default)]
pub struct UpdateReport {
    /// Number of dirty nodes that were evaluated.
    pub nodes_evaluated: usize,
    /// Number of nodes whose output actually changed (hash mismatch).
    pub nodes_changed: usize,
    /// Number of nodes skipped because their output hash was unchanged
    /// (hash-based early exit).
    pub nodes_skipped: usize,
    /// Number of nodes left as Dirty because an upstream source was in
    /// Error state.  These nodes will be retried on the next `update()` call.
    pub nodes_blocked: usize,
    /// Errors encountered during the update, keyed by node ID.
    pub errors: Vec<(NodeId, TransformError)>,
}
impl UpdateReport {
    /// Return `true` if the update completed without any errors.
    pub fn is_ok(&self) -> bool { self.errors.is_empty() }
}
// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------
/// Parallel incremental recomputation engine.
///
/// Owns shared references to the `Graph` (topology + dirty flags) and the
/// `LazyLoader` (value cache + persistence).  Both are wrapped in `Arc` so
/// Tokio tasks can hold clones.
#[derive(Clone)]
pub struct Scheduler {
    graph: Arc<Graph>,
    loader: Arc<LazyLoader>,
}
impl Scheduler {
    /// Create a scheduler over the given graph and loader.
    pub fn new(graph: Arc<Graph>, loader: Arc<LazyLoader>) -> Self {
        Self { graph, loader }
    }
    /// Run one incremental update cycle.
    ///
    /// 1. Computes the topologically-sorted list of dirty nodes.
    /// 2. Decomposes them into parallel waves.
    /// 3. For each wave, spawns one Tokio task per node.
    /// 4. Returns an [`UpdateReport`] when all waves complete.
    pub async fn run_update(&self) -> UpdateReport {
        let topo = self.graph.dirty_nodes_topo();
        if topo.is_empty() {
            return UpdateReport::default();
        }
        let waves = compute_waves(&topo, &self.graph);
        let mut report = UpdateReport::default();
        for wave in waves {
            let handles: Vec<_> = wave.into_iter().map(|nid| {
                let graph = Arc::clone(&self.graph);
                let loader = Arc::clone(&self.loader);
                tokio::spawn(async move {
                    evaluate_node(nid, graph, loader).await
                })
            }).collect();
            let results = join_all(handles).await;
            for result in results {
                match result {
                    Err(join_err) => {
                        // Task panicked – treat as an internal error; we
                        // cannot associate it with a specific node easily.
                        eprintln!("[incremental] task panicked: {join_err}");
                    }
                    Ok(NodeOutcome::Changed(nid)) => {
                        report.nodes_evaluated += 1;
                        report.nodes_changed += 1;
                        let _ = nid;
                    }
                    Ok(NodeOutcome::Unchanged(nid)) => {
                        report.nodes_evaluated += 1;
                        report.nodes_skipped += 1;
                        let _ = nid;
                    }
                    Ok(NodeOutcome::Blocked(nid)) => {
                        // Not evaluated; stays Dirty for the next cycle.
                        report.nodes_blocked += 1;
                        let _ = nid;
                    }
                    Ok(NodeOutcome::InputNode) => {
                        // Input nodes are set externally; not counted as
                        // evaluated by the scheduler.
                    }
                    Ok(NodeOutcome::Error(nid, err)) => {
                        report.nodes_evaluated += 1;
                        report.errors.push((nid, err));
                    }
                }
            }
        }
        report
    }
}
// ---------------------------------------------------------------------------
// Wave decomposition
// ---------------------------------------------------------------------------
/// Split `topo` (already in topological order) into waves.
///
/// Wave 0 = nodes with no dirty predecessors in `topo`.
/// Wave K = nodes whose latest dirty predecessor is in wave K-1.
fn compute_waves(topo: &[NodeId], graph: &Graph) -> Vec<Vec<NodeId>> {
    let mut wave_of: HashMap<NodeId, usize> = HashMap::new();
    for &nid in topo {
        let max_pred_wave = if let Some(edge) = graph.incoming_edge_for(nid) {
            edge.sources.iter()
                .filter_map(|s| wave_of.get(s).copied())
                .max()
                .map(|w| w + 1)
                .unwrap_or(0)
        } else {
            0
        };
        wave_of.insert(nid, max_pred_wave);
    }
    let max_wave = wave_of.values().copied().max().unwrap_or(0);
    let mut waves: Vec<Vec<NodeId>> = vec![vec![]; max_wave + 1];
    for &nid in topo {
        waves[wave_of[&nid]].push(nid);
    }
    waves.retain(|w| !w.is_empty());
    waves
}
// ---------------------------------------------------------------------------
// Per-node evaluation
// ---------------------------------------------------------------------------
enum NodeOutcome {
    Changed(NodeId),
    Unchanged(NodeId),
    InputNode,
    /// Node was left Dirty because an upstream source was in Error state.
    Blocked(NodeId),
    Error(NodeId, TransformError),
}
async fn evaluate_node(
    nid: NodeId,
    graph: Arc<Graph>,
    loader: Arc<LazyLoader>,
) -> NodeOutcome {
    // Input nodes are set externally; mark them clean and move on.
    if graph.is_input(nid) {
        if let Some((v, h)) = graph.peek_value(nid) {
            let _ = graph.store_value(nid, v, h);
        }
        return NodeOutcome::InputNode;
    }
    // Find the edge that feeds this node.
    let edge = match graph.incoming_edge_for(nid) {
        Some(e) => e,
        None => {
            // Computed node with no incoming edge – should not happen in a
            // well-formed graph.
            return NodeOutcome::Error(nid, TransformError::new(
                format!("computed node {nid} has no incoming edge"),
            ));
        }
    };
    // Guard: block evaluation if any source is in a state where its value
    // cannot be trusted:
    //   - Error state: the source's last transform failed; its value (if any)
    //     is stale and should not be consumed.
    //   - Dirty with no value: the source was either also blocked this cycle,
    //     or has never been computed.  Evaluating now would yield a
    //     "source has no value" error that misleadingly looks like a real
    //     transform failure and would poison downstream nodes.
    //
    // In both cases we leave `nid` as Dirty so it is retried on the next
    // update() cycle once all its sources are Clean.
    for &src in &edge.sources {
        let src_status = graph.node_status(src);
        let src_has_value = graph.peek_value(src).is_some()
            || loader.is_cached(src);
        let should_block = match src_status {
            Some(s) if s.is_error() => true,
            Some(s) if s.is_dirty() && !src_has_value => true,
            _ => false,
        };
        if should_block {
            return NodeOutcome::Blocked(nid);
        }
    }
    // Load all source values.
    let mut inputs: Vec<Value> = Vec::with_capacity(edge.sources.len());
    for &src in &edge.sources {
        match loader.get(src).await {
            Ok(Some(v)) => inputs.push(v),
            Ok(None) => {
                // Fall back to graph cache.
                match graph.peek_value(src) {
                    Some((v, _)) => inputs.push(v),
                    None => {
                        return NodeOutcome::Error(nid, TransformError::new(
                            format!("source node {src} has no value"),
                        ));
                    }
                }
            }
            Err(e) => {
                return NodeOutcome::Error(nid, TransformError::with_source(
                    format!("failed to load source {src}"), e.to_string(),
                ));
            }
        }
    }
    // Run the transform.
    let outputs = match edge.transform.apply(&inputs).await {
        Ok(v) => v,
        Err(e) => {
            let _ = graph.store_error(nid, e.clone());
            return NodeOutcome::Error(nid, e);
        }
    };
    // Pair outputs with target node IDs.
    if outputs.len() != edge.targets.len() {
        let err = TransformError::new(format!(
            "transform produced {} outputs but edge has {} targets",
            outputs.len(), edge.targets.len()
        ));
        let _ = graph.store_error(nid, err.clone());
        return NodeOutcome::Error(nid, err);
    }
    let mut any_changed = false;
    for (output, &tid) in outputs.into_iter().zip(edge.targets.iter()) {
        // Hash-based early exit: compare serialised bytes hash against the
        // previously stored hash.  Because Value::to_bytes() uses the serde
        // closure captured at construction time, two logically-equal values
        // produced by separate transform invocations will produce the same
        // hash, correctly skipping downstream recomputation.
        let new_hash = hash_bytes(&loader.registry().serialize_value(&output));
        let prev_hash = graph.last_hash(tid);
        if prev_hash == Some(new_hash) {
            // Hash unchanged – skip persist and downstream dirty.
            let _ = graph.store_value(tid, output, new_hash);
        } else {
            any_changed = true;
            // Store in graph and loader cache.
            let _ = graph.store_value(tid, output.clone(), new_hash);
            loader.cache_value(tid, output);
            // Mark downstream dirty (will be processed in a later wave or
            // the next update() call if they are not already in the current
            // topo sort).
            graph.mark_dirty_downstream(tid);
        }
    }
    if any_changed {
        NodeOutcome::Changed(nid)
    } else {
        NodeOutcome::Unchanged(nid)
    }
}
use std::collections::HashSet;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::loader::LazyLoader;
    use crate::storage::MemoryStorage;
    use crate::transform::{Transform, TypedOneToOne};
    use crate::value::ValueTypeRegistry;
    use std::sync::Arc;

    fn make_registry() -> Arc<ValueTypeRegistry> {
        let mut r = ValueTypeRegistry::new();
        r.register_primitives().unwrap();
        Arc::new(r)
    }

    fn make_double_transform(registry: Arc<ValueTypeRegistry>) -> Transform {
        Transform::OneToOne(Arc::new(TypedOneToOne::new(
            |n: &i32| { let n = *n; async move { Ok(n * 2) } },
            registry,
        )))
    }

    fn make_graph_with_double(registry: Arc<ValueTypeRegistry>) -> (Arc<Graph>, NodeId, NodeId) {
        let g = Arc::new(Graph::new());
        let src = g.add_input_node();
        let tgt = g.add_computed_node();
        g.add_transform(vec![src], vec![tgt], make_double_transform(registry), "double").unwrap();
        (g, src, tgt)
    }

    #[tokio::test]
    async fn scheduler_evaluates_dirty_node() {
        let registry = make_registry();
        let (graph, src, tgt) = make_graph_with_double(Arc::clone(&registry));
        let v = registry.make_value(5i32).unwrap();
        graph.set_input(src, v.clone()).unwrap();
        let storage = Arc::new(MemoryStorage::new());
        let loader = Arc::new(LazyLoader::new(storage, Arc::clone(&registry)));
        loader.cache_value(src, v);
        let scheduler = Scheduler::new(Arc::clone(&graph), Arc::clone(&loader));
        let report = scheduler.run_update().await;
        assert!(report.is_ok(), "errors: {:?}", report.errors);
        assert_eq!(report.nodes_evaluated, 1);
        let (v, _) = graph.peek_value(tgt).expect("target should have value");
        assert_eq!(registry.downcast_value::<i32>(&v, "test").unwrap(), 10i32);
    }

    #[tokio::test]
    async fn empty_graph_returns_empty_report() {
        let registry = make_registry();
        let graph = Arc::new(Graph::new());
        let storage = Arc::new(MemoryStorage::new());
        let loader = Arc::new(LazyLoader::new(storage, registry));
        let scheduler = Scheduler::new(graph, loader);
        let report = scheduler.run_update().await;
        assert_eq!(report.nodes_evaluated, 0);
    }

    #[test]
    fn compute_waves_single_chain() {
        let registry = make_registry();
        let g = Arc::new(Graph::new());
        let a = g.add_input_node();
        let b = g.add_computed_node();
        let c = g.add_computed_node();
        g.add_transform(vec![a], vec![b], make_double_transform(Arc::clone(&registry)), "ab").unwrap();
        g.add_transform(vec![b], vec![c], make_double_transform(Arc::clone(&registry)), "bc").unwrap();
        let topo = g.dirty_nodes_topo();
        let waves = compute_waves(&topo, &g);
        assert!(waves.len() >= 2, "chain should have at least 2 waves");
        let wave0_ids: HashSet<_> = waves[0].iter().copied().collect();
        assert!(wave0_ids.contains(&a));
    }
}
