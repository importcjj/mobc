use std::future::Future;

enum Runtime {
    #[cfg(feature = "tokio-comp")]
    Tokio,
    #[cfg(feature = "async-std-comp")]
    AsyncStd,
    #[cfg(feature = "actix-comp")]
    Actix,
}

/// Spawns a new asynchronous task.
pub fn spawn<T>(task: T)
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    #[cfg(all(
        not(feature = "tokio-comp"),
        not(feature = "async-std-comp"),
        not(feature = "actix-comp"),
    ))]
    {
        compile_error!("tokio-comp, async-std-comp or actix-comp feature required")
    }

    #[cfg(feature = "tokio-comp")]
    let rt = Runtime::Tokio;

    #[cfg(feature = "async-std-comp")]
    let rt = Runtime::AsyncStd;

    #[cfg(feature = "actix-comp")]
    let rt = Runtime::Actix;

    #[cfg(any(
        feature = "tokio-comp",
        feature = "async-std-comp",
        feature = "actix-comp",
    ))]
    match rt {
        #[cfg(feature = "tokio-comp")]
        Runtime::Tokio => tokio::spawn(task),
        #[cfg(feature = "async-std-comp")]
        Runtime::AsyncStd => async_std::task::spawn(task),
        #[cfg(feature = "actix-comp")]
        Runtime::Actix => actix_rt::spawn(async {
            task.await;
        }),
    };
}
