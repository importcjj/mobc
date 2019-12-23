pub use time::{interval, timeout};

#[cfg(all(
    feature = "tokio",
    not(any(feature = "tokio-02-alpha6", feature = "async-std"))
))]
mod time {
    pub use tokio::time::delay_for as timeout;
    pub use tokio::time::interval;
}

#[cfg(all(feature = "tokio-02-alpha6", not(feature = "async-std")))]
mod time {
    use std::time::Duration;
    use std::time::Instant;
    pub use tokio_timer::delay_for as timeout;

    pub fn interval(duration: Duration) -> Interval {
        Interval(tokio_timer::Interval::new_interval(duration))
    }

    pub struct Interval(tokio_timer::Interval);

    impl Interval {
        pub async fn tick(&mut self) -> Instant {
            self.0.next().await;
            Instant::now()
        }
    }
}

#[cfg(all(feature = "async-std", not(feature = "tokio-02-alpha6")))]
mod time {
    use std::future::Future;
    use std::time::Duration;
    use std::time::Instant;
    pub struct Interval(async_std::stream::Interval);

    impl Interval {
        pub async fn tick(&mut self) -> Instant {
            use futures::StreamExt;
            self.0.next().await;
            Instant::now()
        }
    }

    pub fn interval(duration: Duration) -> Interval {
        Interval(async_std::stream::interval(duration))
    }

    pub fn timeout(duration: Duration) -> impl Future<Output = ()> {
        use futures::FutureExt;
        let fut = futures::future::pending::<()>();
        Box::pin(async_std::future::timeout(duration, fut).map(|_| ()))
    }
}
