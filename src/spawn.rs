use std::future::Future;
use tracing::instrument::WithSubscriber;
use tracing::Dispatch;

/// Spawns a new asynchronous task.
pub fn spawn<T>(task: T)
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    #[cfg(all(
        feature = "tokio",
        not(any(feature = "async-std", feature = "actix-rt"))
    ))]
    spawn_tokio(task);

    #[cfg(all(feature = "async-std", not(feature = "actix-rt")))]
    async_std::task::spawn(task);

    #[cfg(all(feature = "actix-rt", not(feature = "async-std")))]
    actix_rt::spawn(async {
        task.await;
    });
}

#[cfg(all(
    feature = "tokio",
    not(any(feature = "async-std", feature = "actix-rt"))
))]
pub fn spawn_tokio<T>(task: T)
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    let dispatcher = get_current_dispatcher();
    tokio::spawn(task.with_subscriber(dispatcher));
}

#[cfg(all(
    feature = "tokio",
    not(any(feature = "async-std", feature = "actix-rt"))
))]
pub fn get_current_dispatcher() -> Dispatch {
    tracing::dispatcher::get_default(|current| current.clone())
}
