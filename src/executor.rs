//! A value that executes futures.
use futures::Future;
use std::error::Error;
use std::fmt;
use std::pin::Pin;

// A new type exports the default executor of Tokio..
// pub type DefaultExecutor = DefaultExecutor;

/// A value that executes futures.
/// see [tokio::Executor](https://docs.rs/tokio/0.2.0-alpha.6/tokio/executor/trait.Executor.html)
pub trait Executor: Send + Sync + 'static + Clone {
    /// Spawns a future object to run on this executor.
    ///
    /// `future` is passed to the executor, which will begin running it. The
    /// future may run on the current thread or another thread at the discretion
    /// of the `Executor` implementation.
    fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>)
        -> Result<(), SpawnError>;

    /// Provides a best effort **hint** to whether or not `spawn` will succeed.
    ///
    /// This function may return both false positives **and** false negatives.
    /// If `status` returns `Ok`, then a call to `spawn` will *probably*
    /// succeed, but may fail. If `status` returns `Err`, a call to `spawn` will
    /// *probably* fail, but may succeed.
    ///
    /// This allows a caller to avoid creating the task if the call to `spawn`
    /// has a high likelihood of failing.
    fn status(&self) -> Result<(), SpawnError> {
        Ok(())
    }
}

#[derive(Debug)]
/// Errors returned by Executor::spawn.
/// Spawn errors should represent relatively rare scenarios. Currently, the two scenarios represented by SpawnError are:
/// An executor being at capacity or full. As such, the executor is not able to accept a new future. This error state is expected to be transient.
/// An executor has been shutdown and can no longer accept new futures. This error state is expected to be permanent.
pub struct SpawnError {
    pub(crate) is_shutdown: bool,
}

impl fmt::Display for SpawnError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            fmt,
            "attempted to spawn task while the executor is at capacity or shut down"
        )
    }
}

impl Error for SpawnError {}
