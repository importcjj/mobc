pub use time::{delay_for, delay_until, interval};

#[cfg(all(
    feature = "tokio",
    not(any(feature = "tokio-02-alpha6", feature = "async-std"))
))]
mod time {
    use std::time::Instant;
    pub use tokio::time::delay_for;
    pub use tokio::time::interval;
    pub use tokio::time::Delay;

    pub fn delay_until(deadline: Instant) -> Delay {
        tokio::time::delay_until(tokio::time::Instant::from_std(deadline))
    }
}

// #[cfg(all(feature = "tokio-02-alpha6", not(feature = "async-std")))]
// mod time {
//     use std::time::Duration;
//     use std::time::Instant;
//     pub use tokio02alpha6::timer::delay as delay_until;
//     pub use tokio02alpha6::timer::delay_for;

//     pub fn interval(duration: Duration) -> Interval {
//         Interval(tokio02alpha6::timer::Interval::new_interval(duration))
//     }

//     pub struct Interval(tokio02alpha6::timer::Interval);

//     impl Interval {
//         pub async fn tick(&mut self) -> Instant {
//             self.0.next().await;
//             Instant::now()
//         }
//     }
// }

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

    pub fn delay_for(duration: Duration) -> impl Future<Output = ()> {
        use futures::FutureExt;
        let fut = futures::future::pending::<()>();
        Box::pin(async_std::future::timeout(duration, fut).map(|_| ()))
    }

    pub fn delay_until(deadline: Instant) -> impl Future<Output = ()> {
        use futures::FutureExt;
        let fut = futures::future::pending::<()>();
        let now = Instant::now();
        let duration = if now > deadline {
            Duration::from_secs(0)
        } else {
            deadline - now
        };
        Box::pin(async_std::future::timeout(duration, fut).map(|_| ()))
    }
}
