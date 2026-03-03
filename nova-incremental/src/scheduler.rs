//! Wave-parallel task scheduler.
//!
//! The scheduler knows nothing about the graph, values, or state.
//! It receives a [`TaskGraph`] (pre-loaded, self-contained tasks), partitions
//! them into dependency-ordered waves, executes each wave in parallel via
//! `tokio::spawn`, and returns all [`TaskResult`]s to the caller.
//!
//! All state mutation (WorkState, Loader) is handled by [`ExecutionContext`].

use std::collections::HashMap;
use uuid::Uuid;
use crate::execution_context::{TaskGraph, ReadyTask, TaskResult};
use crate::node_id::NodeId;
use crate::transform::TransformError;

// ---------------------------------------------------------------------------
// UpdateReport (public)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct UpdateReport {
    pub transforms_evaluated:        usize,
    pub transforms_changed:          usize,
    pub transforms_skipped:          usize,
    pub transforms_blocked:          usize,
    pub collection_elements_changed: usize,
    pub errors:                      Vec<(Uuid, TransformError)>,
    pub cycle_limit_exceeded:        Vec<Uuid>,
}

impl UpdateReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty() && self.cycle_limit_exceeded.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

pub(crate) struct Scheduler {
    pub(crate) cycle_limit: u32,
}

impl Scheduler {
    pub(crate) fn new() -> Self { Self { cycle_limit: 1000 } }

    /// Execute all tasks in the TaskGraph in dependency-wave order.
    /// Returns all results; does NOT touch WorkState or Loader.
    pub(crate) async fn run(&self, task_graph: TaskGraph) -> Vec<TaskResult> {
        let waves = compute_waves(task_graph.tasks);
        let mut all_results = Vec::new();

        for wave in waves {
            let mut handles = Vec::with_capacity(wave.len());
            for task in wave {
                handles.push(tokio::spawn(async move { task.execute().await }));
            }
            for handle in handles {
                match handle.await {
                    Ok(r)  => all_results.push(r),
                    Err(e) => all_results.push(TaskResult {
                        id:          NodeId::new(),
                        outputs:     vec![],
                        fanout_key:  None,
                        fanout_slot: None,
                        error:       Some(TransformError::new(e.to_string())),
                    }),
                }
            }
        }

        all_results
    }
}

// ---------------------------------------------------------------------------
// Wave computation
// ---------------------------------------------------------------------------

/// Partition tasks into dependency-ordered waves.
/// Tasks in the same wave have no dependencies on each other.
fn compute_waves(tasks: Vec<ReadyTask>) -> Vec<Vec<ReadyTask>> {
    // For fan-out tasks (multiple ReadyTask per NodeId), group by id first
    // so all elements of the same transform are in the same wave.
    let dirty_set: std::collections::HashSet<NodeId> = tasks.iter().map(|t| t.id).collect();

    // Compute wave level for each unique NodeId.
    let mut node_wave: HashMap<NodeId, usize> = HashMap::new();
    for t in &tasks {
        if node_wave.contains_key(&t.id) { continue; }
        let w = t.dependencies.iter()
            .filter(|d| dirty_set.contains(*d))
            .map(|d| node_wave.get(d).copied().unwrap_or(0) + 1)
            .max()
            .unwrap_or(0);
        node_wave.insert(t.id, w);
    }

    let max_wave = node_wave.values().copied().max().unwrap_or(0);
    let mut waves: Vec<Vec<ReadyTask>> = (0..=max_wave).map(|_| Vec::new()).collect();

    for task in tasks {
        let w = node_wave.get(&task.id).copied().unwrap_or(0);
        waves[w].push(task);
    }

    waves.retain(|w| !w.is_empty());
    waves
}