//! [`TaskQueue`] trait and [`SequentialTaskQueue`] default implementation.
//!
//! The `TaskQueue` controls how the engine schedules and executes transforms.
//! Provide a custom implementation (e.g. thread-pool based) via
//! [`EngineBuilder::with_task_queue`].
//!
//! ## drain() semantics
//!
//! `drain()` runs all enqueued tasks until the queue reaches a *stable empty
//! state*: the queue is empty AND no task is currently executing (meaning no
//! further enqueues can come from in-flight work).
//!
//! Enqueuing from inside a running task is explicitly allowed — that is the
//! mechanism by which dirty propagation triggers downstream work.
//!
//! ## Send safety
//!
//! `TaskQueue: Send + Sync`. Task futures passed to `enqueue` must be
//! `Send + 'static`, which ensures [`Engine`](crate::Engine) is also `Send`
//! and can be used with `tokio::spawn`.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// A heap-boxed `Send` future with no output.
pub(crate) type SendBoxFuture = Pin<Box<dyn Future<Output=()> + Send + 'static>>;

// ---------------------------------------------------------------------------
// TaskQueue trait
// ---------------------------------------------------------------------------

/// Controls task scheduling for an incremental engine.
///
/// The engine calls [`TaskQueue::enqueue`] whenever a node becomes pending,
/// and calls [`TaskQueue::drain`] once per [`Engine::update`](crate::Engine::update) call.
///
/// # Implementing `TaskQueue`
///
/// - **`enqueue`** receives a ready-to-run `Send + 'static` future. The implementation
///   may store it for later execution (sequential) or spawn it immediately (parallel).
/// - **`drain`** returns a `Send` future that resolves when all queued and
///   in-flight work is complete (stable empty state).
///
/// # `Send + Sync` requirement
///
/// Both the trait and its enqueued futures are `Send + 'static`, which makes
/// [`Engine`](crate::Engine) `Send` and compatible with `tokio::spawn`.
pub trait TaskQueue: Send + Sync {
    /// Enqueue a `Send + 'static` future for execution.
    /// May be called from within a running task.
    fn enqueue(&self, fut: SendBoxFuture);

    /// Run all enqueued tasks until the queue reaches a stable empty state.
    ///
    /// Returns a `Send + 'static` future. The future resolves only when both:
    /// (a) the queue is empty, and (b) no task is currently executing.
    ///
    /// In a sequential implementation, newly-enqueued work is processed
    /// before returning. In a parallel implementation, an in-flight counter
    /// must be used to detect stable empty state.
    fn drain(&self) -> SendBoxFuture;
}

// ---------------------------------------------------------------------------
// SequentialTaskQueue
// ---------------------------------------------------------------------------

/// Default single-threaded FIFO task queue.
///
/// Tasks are executed one at a time in enqueue order. Any task that enqueues
/// further work is processed before `drain()` returns.
///
/// Thread-safe (`Send + Sync`) — may be wrapped in `Arc` and shared.
pub struct SequentialTaskQueue {
    queue: Arc<Mutex<VecDeque<SendBoxFuture>>>,
}

impl SequentialTaskQueue {
    /// Create a new empty sequential task queue.
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

impl Default for SequentialTaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskQueue for SequentialTaskQueue {
    fn enqueue(&self, fut: SendBoxFuture) {
        // std::sync::Mutex is always safe here: the lock is never held during
        // future execution (see drain() below).
        self.queue.lock().unwrap().push_back(fut);
    }

    fn drain(&self) -> SendBoxFuture {
        let queue = Arc::clone(&self.queue);
        Box::pin(async move {
            loop {
                // Lock briefly to pop one task, then release before executing.
                // This allows enqueue() (which also locks) to be called from
                // within the executing task.
                let task = queue.lock().unwrap().pop_front();
                match task {
                    None => break, // stable empty state
                    Some(fut) => {
                        fut.await;
                    }
                }
            }
        })
    }
}
