#[allow(unused_imports)]
use futures::Future;
use std::time::Duration;
use std::time::Instant;

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub(crate) fn interval(dur: Duration) -> tokio::timer::Interval {
    tokio::timer::Interval::new_interval(dur)
}

#[cfg(feature = "async-std-runtime")]
pub(crate) fn interval(dur: Duration) -> async_std::stream::Interval {
    async_std::stream::interval(dur)
}

#[cfg(feature = "tokio-runtime")]
#[cfg(not(feature = "async-std-runtime"))]
pub(crate) fn timeout(dur: Duration) -> tokio::timer::Delay {
    let now = Instant::now();
    tokio::timer::delay(now + dur)
}

#[cfg(feature = "async-std-runtime")]
pub(crate) fn timeout(dur: Duration) -> impl Future<Output = ()> {
    use futures::FutureExt;

    let fut = futures::future::pending::<()>();
    Box::pin(async_std::future::timeout(dur, fut).map(|_| ()))
}
