//! A batteries included runtime for applications using mobc.
//! Mobc does not implement runtime, it simply exports runtime.

pub use runtime::{DefaultExecutor, Runtime, TaskExecutor};

use futures::Future;
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

#[cfg(all(
    feature = "tokio",
    not(any(feature = "tokio-02-alpha6-global", feature = "async-std"))
))]
mod runtime {
    use super::*;
    pub use tokio::runtime::Handle as TaskExecutor;
    pub use tokio::runtime::Runtime;

    impl Executor for tokio::runtime::Handle {
        fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
            tokio::runtime::Handle::spawn(self, future);
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

// #[cfg(all(feature = "tokio-02-alpha6-global", not(feature = "async-std")))]
// mod runtime {
//     use super::*;
//     pub use tokio02alpha6::excutor::DefaultExecutor;
//     pub use tokio02alpha6::runtime::Runtime;
//     pub use tokio02alpha6::runtime::TaskExecutor;

//     impl<T> Executor for T
//     where
//         T: tokio02alpha6::executor::Executor + Send + Sync + 'static + Clone,
//     {
//         fn spawn(&mut self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
//             tokio02alpha6::executor::Executor::spawn(self, future);
//         }
//     }
// }

#[cfg(all(feature = "async-std", not(feature = "tokio-02-alpha6-global")))]
mod runtime {
    use super::*;
    use async_std::task;
    use async_std::task::JoinHandle;
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
