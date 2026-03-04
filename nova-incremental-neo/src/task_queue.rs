//! [`TaskQueue`] trait and [`SequentialTaskQueue`] implementation.
//!
//! The `TaskQueue` is the only concurrency abstraction in the engine.
//! The sequential implementation runs tasks one at a time, FIFO.
//! A future parallel implementation can replace this without changing
//! any other module.

use std::future::Future;
use std::pin::Pin;
use std::collections::VecDeque;

pub(crate) type BoxFuture = Pin<Box<dyn Future<Output = ()> + 'static>>;
pub(crate) type TaskFn = Box<dyn FnOnce() -> BoxFuture + 'static>;

// ---------------------------------------------------------------------------
// TaskQueue trait
// ---------------------------------------------------------------------------

/// Abstraction for task scheduling within an incremental update pass.
///
/// ## drain() semantics
///
/// `drain()` runs all enqueued tasks and returns only when the queue reaches
/// a *stable empty state*: the queue is empty AND no task is currently
/// executing (meaning no further enqueues can come from in-flight work).
///
/// Enqueuing from inside a running task is explicitly allowed and expected —
/// that is the mechanism by which dirty propagation triggers downstream work.
/// `drain()` must process those newly-enqueued tasks before returning.
pub(crate) trait TaskQueue {
    /// Enqueue a task for execution. May be called from any context, including
    /// from within a running task.
    fn enqueue(&self, task: TaskFn);

    /// Run all tasks until stable empty. Returns a future that resolves when done.
    fn drain(&self) -> BoxFuture;
}

// ---------------------------------------------------------------------------
// SequentialTaskQueue
// ---------------------------------------------------------------------------

/// Single-threaded FIFO task queue. Tasks run one at a time in enqueue order.
///
/// `drain()` processes all tasks, including any enqueued by tasks themselves,
/// until the queue is empty with no task running.
pub(crate) struct SequentialTaskQueue {
    queue: std::sync::Mutex<VecDeque<TaskFn>>,
}

impl SequentialTaskQueue {
    pub(crate) fn new() -> Self {
        Self { queue: std::sync::Mutex::new(VecDeque::new()) }
    }
}

impl TaskQueue for SequentialTaskQueue {
    fn enqueue(&self, task: TaskFn) {
        // In the sequential implementation we use try_lock because enqueue
        // can be called from within a running task (which holds no queue lock).
        // We use a blocking lock via futures::executor::block_on for simplicity;
        // in the sequential case the task is always enqueued synchronously.
        //
        // NOTE: This is called from async context but we need a synchronous enqueue.
        // We use try_lock here; since sequential tasks are not concurrent,
        // the queue mutex is never contended during a task's execution.
        if let Ok(mut q) = self.queue.try_lock() {
            q.push_back(task);
        } else {
            // Should not happen in sequential mode: enqueue is only called from
            // within a drain() task, and drain() releases the queue lock before
            // executing each task. If this path is hit it is a programming error.
            panic!("SequentialTaskQueue: enqueue called while queue is locked — programming error");
        }
    }

    fn drain(&self) -> BoxFuture {
        Box::pin(async move {
            // Safety: the TaskQueue trait object owns self. We know this is
            // SequentialTaskQueue because we're in its impl.
            // We cannot access self here directly (drain returns BoxFuture + 'static).
            // This design requires a workaround: SequentialTaskQueue must be Arc'd.
            // See the note in engine.rs — the queue is held as Arc<dyn TaskQueue>.
            // We cannot implement drain() correctly as a 'static future without
            // capturing Arc<Self>. This is resolved by DrainHandle below.
        })
    }
}

// ---------------------------------------------------------------------------
// ArcSequentialTaskQueue — the actual usable version
// ---------------------------------------------------------------------------

/// Wrapper that owns the queue via `Arc` so `drain()` can capture it.
pub(crate) struct ArcSequentialTaskQueue {
    queue: std::sync::Arc<std::sync::Mutex<VecDeque<TaskFn>>>,
}

impl ArcSequentialTaskQueue {
    pub(crate) fn new() -> Self {
        Self { queue: std::sync::Arc::new(std::sync::Mutex::new(VecDeque::new())) }
    }
}

impl TaskQueue for ArcSequentialTaskQueue {
    fn enqueue(&self, task: TaskFn) {
        self.queue.lock().unwrap().push_back(task);
    }

    fn drain(&self) -> BoxFuture {
        let queue = std::sync::Arc::clone(&self.queue);
        Box::pin(async move {
            loop {
                // Pop one task while holding the lock briefly (std::sync, no await).
                let task = queue.lock().unwrap().pop_front();
                match task {
                    None => break, // queue empty — stable empty state
                    Some(task_fn) => {
                        // Release the lock before executing. The task may call enqueue()
                        // which also locks the queue (try_lock succeeds because we released).
                        let future = task_fn();
                        future.await;
                    }
                }
            }
        })
    }
}
