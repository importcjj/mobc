use futures::FutureExt;
use futures::StreamExt;
#[allow(unused_imports)]
use std::future::Future;
use std::time::Duration;
use std::time::Instant;

#[cfg(feature = "async-std-runtime")]
pub struct AsyncInterval(async_std::stream::Interval);

#[cfg(feature = "async-std-runtime")]
impl AsyncInterval {
    pub async fn tick(&mut self) -> Instant {
        self.0.next().await;
        Instant::now()
    }
}

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub(crate) fn interval(dur: Duration) -> tokio::time::Interval {
    tokio::time::interval(dur)
}

#[cfg(feature = "async-std-runtime")]
pub(crate) fn interval(dur: Duration) -> AsyncInterval {
    AsyncInterval(async_std::stream::interval(dur))
}

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub(crate) fn timeout(dur: Duration) -> impl Future<Output = ()> {
    let fut = futures::future::pending::<()>();
    Box::pin(tokio::time::timeout(dur, fut).map(|_| ()))
}

#[cfg(feature = "async-std-runtime")]
pub(crate) fn timeout(dur: Duration) -> impl Future<Output = ()> {
    let fut = futures::future::pending::<()>();
    Box::pin(async_std::future::timeout(dur, fut).map(|_| ()))
}
