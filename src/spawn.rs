pub use spawn::spawn;

#[cfg(all(
    feature = "tokio",
    not(any(feature = "tokio-02-alpha6-global", feature = "async-std"))
))]
mod spawn {
    pub use tokio::spawn;
}

#[cfg(all(feature = "tokio-02-alpha6-global", not(feature = "async-std")))]
mod spawn {
    pub use tokio_executor::spawn;
}

#[cfg(all(feature = "async-std", not(feature = "tokio-02-alpha6-global")))]
mod spawn {
    pub use async_std::task::spawn;
}
