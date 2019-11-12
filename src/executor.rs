use futures::Future;
use std::error::Error;
use std::fmt;
use std::pin::Pin;
use tokio_executor::Executor as TkExecutor;

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

impl<T> Executor for T
where
    T: TkExecutor + Send + Sync + 'static + Clone,
{
    fn spawn(
        &mut self,
        future: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> Result<(), SpawnError> {
        match TkExecutor::spawn(self, future) {
            Ok(()) => Ok(()),
            Err(err) => Err(SpawnError {
                is_shutdown: err.is_shutdown(),
            }),
        }
    }

    fn status(&self) -> Result<(), SpawnError> {
        match TkExecutor::status(self) {
            Ok(()) => Ok(()),
            Err(err) => Err(SpawnError {
                is_shutdown: err.is_shutdown(),
            }),
        }
    }
}

#[derive(Debug)]
pub struct SpawnError {
    is_shutdown: bool,
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
