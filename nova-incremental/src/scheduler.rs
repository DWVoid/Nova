//! Simple sequential task scheduler for the incremental engine.
//!
//! Tasks are submitted in topological order by `run_pass`, so running them
//! sequentially is always correct.  Concurrency can be added later.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public: UpdateReport
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct UpdateReport {
    pub transforms_evaluated:        usize,
    pub transforms_changed:          usize,
    pub transforms_skipped:          usize,
    pub transforms_blocked:          usize,
    pub collection_elements_changed: usize,
    pub errors:                      Vec<(Uuid, crate::transform::TransformError)>,
    pub cycle_limit_exceeded:        Vec<Uuid>,
}

impl UpdateReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty() && self.cycle_limit_exceeded.is_empty()
    }
}

// ---------------------------------------------------------------------------
// TaskId — opaque handle (unused for ordering; topo order suffices)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TaskId(u64);

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

type BoxFuture = std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub(crate) struct Scheduler {
    counter: AtomicU64,
    queue:   Mutex<Vec<BoxFuture>>,
    pub(crate) cycle_limit: u32,
}

impl Scheduler {
    pub(crate) fn new() -> Self {
        Self {
            counter:     AtomicU64::new(1),
            queue:       Mutex::new(vec![]),
            cycle_limit: 1000,
        }
    }

    /// Enqueue a task with no dependencies.
    pub(crate) async fn spawn(&self, f: impl Future<Output = ()> + Send + 'static) -> TaskId {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        self.queue.lock().await.push(Box::pin(f));
        TaskId(id)
    }

    /// Enqueue a task after its dependencies.  Since tasks are submitted in
    /// topological order, declared deps have always been enqueued already.
    pub(crate) async fn schedule(
        &self,
        _deps: &[TaskId],
        f: impl Future<Output = ()> + Send + 'static,
    ) -> TaskId {
        // Deps already guaranteed to run earlier in topo order.
        self.spawn(f).await
    }

    /// Run all queued tasks sequentially, then clear the queue.
    pub(crate) async fn join(&self) {
        let tasks: Vec<BoxFuture> = self.queue.lock().await.drain(..).collect();
        for task in tasks {
            task.await;
        }
    }
}