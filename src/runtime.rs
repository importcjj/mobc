//! A batteries included runtime for applications using mobc.
//! Mobc does not implement runtime, it simply exports runtime from:
//! * [Tokio](https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html)
//! * [async-std](https://docs.rs/async-std/latest/async_std/task/index.html)
//! More examples, see [here](https://github.com/importcjj/mobc/tree/master/postgres/examples)
use crate::executor::SpawnError;
use crate::Executor;
use futures::Future;

use std::pin::Pin;


#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub use tokio::executor::DefaultExecutor;
#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub use tokio::runtime::Runtime;
#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub use tokio::runtime::TaskExecutor;
#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub use tokio::timer::delay_for;

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
impl<T> Executor for T
where
    T: tokio::executor::Executor + Send + Sync + 'static + Clone,
{
    fn spawn(
        &mut self,
        future: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> Result<(), SpawnError> {
        match tokio::executor::Executor::spawn(self, future) {
            Ok(()) => Ok(()),
            Err(err) => Err(SpawnError {
                is_shutdown: err.is_shutdown(),
            }),
        }
    }

    fn status(&self) -> Result<(), SpawnError> {
        match tokio::executor::Executor::status(self) {
            Ok(()) => Ok(()),
            Err(err) => Err(SpawnError {
                is_shutdown: err.is_shutdown(),
            }),
        }
    }
}

#[cfg(feature = "async-std-runtime")]
pub struct Runtime;

#[cfg(feature = "async-std-runtime")]
impl Runtime {
    pub fn new() -> Option<Self> {
        Some(Self)
    }

    pub fn executor(&self) -> TaskExecutor {
        TaskExecutor {}
    }

    pub fn block_on<F, T>(&self, future: F) -> T
    where
        F: Future<Output = T>,
    {
        async_std::task::block_on(future)
    }

    pub fn spawn<F, T>(&self, future: F)
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        async_std::task::spawn(future);
    }
}

#[cfg(feature = "async-std-runtime")]
#[derive(Clone)]
pub struct TaskExecutor;

#[cfg(feature = "async-std-runtime")]
impl Executor for TaskExecutor {
    fn spawn(
        &mut self,
        future: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> Result<(), SpawnError> {
        async_std::task::spawn(future);
        Ok(())
    }
}

#[cfg(feature = "async-std-runtime")]
#[derive(Clone)]
pub struct DefaultExecutor;

#[cfg(feature = "async-std-runtime")]
impl DefaultExecutor {
    pub fn current() -> Self {
        Self {}
    }
}

#[cfg(feature = "async-std-runtime")]
impl Executor for DefaultExecutor {
    fn spawn(
        &mut self,
        future: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> Result<(), SpawnError> {
        async_std::task::spawn(future);
        Ok(())
    }
}

#[cfg(feature = "async-std-runtime")]
pub fn delay_for(dur: Duration) -> impl Future<Output = ()> {
    use futures::FutureExt;

    let fut = futures::future::pending::<()>();
    Box::pin(async_std::future::timeout(dur, fut).map(|_| ()))
}
