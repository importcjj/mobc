//! A value that executes futures.
use futures::Future;
use std::pin::Pin;

/// A handle that awaits the result of a task.
/// Dropping a JoinHandle will detach the task, meaning that there is no longer a handle to the task and no way to join on it.
/// Created when a task is spawned.
pub type JoinHandle = Box<dyn Future<Output = ()> + Send>;

/// A value that executes futures.
/// see [tokio::Executor](https://docs.rs/tokio/0.2.0-alpha.6/tokio/executor/trait.Executor.html)
pub trait Executor: Send + Sync + 'static + Clone {
    /// Spawns a future object to run on this executor.
    ///
    /// `future` is passed to the executor, which will begin running it. The
    /// future may run on the current thread or another thread at the discretion
    /// of the `Executor` implementation.
    fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) -> JoinHandle;
}
