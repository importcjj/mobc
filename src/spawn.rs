use std::future::Future;

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
    tokio::spawn(task);

    #[cfg(all(feature = "async-std", not(feature = "actix-rt")))]
    async_std::task::spawn(task);

    #[cfg(all(feature = "actix-rt", not(feature = "async-std")))]
    actix_rt::spawn(async {
        task.await;
    });
}
