//! A batteries included runtime for applications using mobc.
//! Mobc does not implement runtime, it simply exports runtime.

pub use runtime::{DefaultExecutor, Runtime, TaskExecutor};

use std::future::Future;
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
    fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>);
}

#[cfg(all(feature = "tokio", not(feature = "async-std")))]
mod runtime {
    use super::*;

    /// Wrapper of the Tokio Runtime
    pub struct Runtime {
        rt: tokio::runtime::Runtime,
        spawner: TaskExecutor,
    }

    impl Runtime {
        /// Creates a new Runtime
        pub fn new() -> Option<Self> {
            Some(Runtime {
                rt: tokio::runtime::Runtime::new().unwrap(),
                spawner: TaskExecutor,
            })
        }

        /// Returns a spawner
        pub fn handle(&self) -> &TaskExecutor {
            &self.spawner
        }

        /// Run a future to completion on the Tokio runtime. This is the
        /// runtime's entry point.
        pub fn block_on<F, T>(&mut self, future: F) -> T
        where
            F: Future<Output = T>,
        {
            self.rt.block_on(future)
        }

        /// Spawn a future onto the Tokio runtime.
        pub fn spawn<F, T>(&self, future: F)
        where
            F: Future<Output = T> + Send + 'static,
            T: Send + 'static,
        {
            self.rt.spawn(future);
        }
    }

    /// Simple handler for spawning task
    #[derive(Clone)]
    pub struct TaskExecutor;

    impl TaskExecutor {
        /// Spawn a future onto the Tokio runtime.
        pub fn spawn<F>(&self, future: F)
        where
            F: Future + Send + 'static,
            F::Output: Send + 'static,
        {
            tokio::spawn(future);
        }
    }

    #[derive(Clone)]
    /// The default executor of tokio.
    pub struct DefaultExecutor;

    impl DefaultExecutor {
        /// The default executor of tokio.
        pub fn current() -> Self {
            Self {}
        }
    }

    impl Executor for DefaultExecutor {
        fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
            tokio::spawn(future);
        }
    }
}

#[cfg(all(feature = "async-std"))]
mod runtime {
    use super::*;
    use async_std::task;

    #[derive(Clone)]
    pub struct TaskExecutor;

    impl TaskExecutor {
        pub fn spawn<F>(&self, future: F)
        where
            F: Future + Send + 'static,
            F::Output: Send + 'static,
        {
            task::spawn(future);
        }
    }

    pub struct Runtime(TaskExecutor);

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
            task::block_on(future)
        }

        pub fn spawn<F, T>(&self, future: F)
        where
            F: Future<Output = T> + Send + 'static,
            T: Send + 'static,
        {
            task::spawn(future);
        }
    }

    #[derive(Clone)]
    pub struct DefaultExecutor;

    impl DefaultExecutor {
        pub fn current() -> Self {
            Self {}
        }
    }

    impl Executor for DefaultExecutor {
        fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
            task::spawn(future);
        }
    }
}
