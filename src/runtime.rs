//! A batteries included runtime for applications using mobc.
//! Mobc does not implement runtime, it simply exports runtime from:
//! * [Tokio](https://docs.rs/tokio/latest/tokio/runtime/struct.Runtime.html)
//! * [async-std](https://docs.rs/async-std/latest/async_std/task/index.html)
//! More examples, see [here](https://github.com/importcjj/mobc/tree/master/postgres/examples)
use crate::executor::JoinHandle;
use crate::Executor;
use futures::FutureExt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub use tokio::runtime::Runtime;

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub use tokio::runtime::Handle as TaskExecutor;

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub use tokio::time::delay_for;

#[cfg(feature = "async-std-runtime")]
pub struct Runtime(TaskExecutor);

#[cfg(feature = "async-std-runtime")]
impl Runtime {
    pub fn new() -> Option<Self> {
        Some(Runtime(TaskExecutor))
    }

    pub fn handle(&self) -> &TaskExecutor {
        &self.0
    }

    pub fn block_on<F, T>(&mut self, future: F) -> T
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

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
impl Executor for tokio::runtime::Handle {
    fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) -> JoinHandle {
        let join = tokio::runtime::Handle::spawn(self, future);

        Box::new(join.map(|_| ()))
    }
}

#[cfg(feature = "async-std-runtime")]
#[derive(Clone)]
pub struct TaskExecutor;

#[cfg(feature = "async-std-runtime")]
impl TaskExecutor {
    pub fn spawn<F>(&self, future: F) -> async_std::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        async_std::task::spawn(future)
    }
}

#[cfg(feature = "async-std-runtime")]
impl Executor for TaskExecutor {
    fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) -> JoinHandle {
        let join = async_std::task::spawn(future);
        Box::new(join)
    }
}

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
#[derive(Clone)]
/// The default executor of tokio.
pub struct DefaultExecutor;

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
impl DefaultExecutor {
    /// The default executor of tokio.
    pub fn current() -> Self {
        Self {}
    }
}

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
impl Executor for DefaultExecutor {
    fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) -> JoinHandle {
        let join = tokio::spawn(future);
        Box::new(join.map(|_| ()))
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
    fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) -> JoinHandle {
        let join = async_std::task::spawn(future);
        Box::new(join)
    }
}

#[cfg(feature = "async-std-runtime")]
pub fn delay_for(dur: Duration) -> impl Future<Output = ()> {
    use futures::FutureExt;

    let fut = futures::future::pending::<()>();
    Box::pin(async_std::future::timeout(dur, fut).map(|_| ()))
}
