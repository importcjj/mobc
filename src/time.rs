use crate::Error;
use futures::FutureExt;
use std::future::Future;
use std::time::Duration;
pub use time::{delay_for, delay_until, interval};

pub(crate) async fn timeout<F, T, E>(duration: Duration, f: F) -> Result<T, Error<E>>
where
    F: Future<Output = Result<T, Error<E>>>,
{
    futures::select! {
        () = delay_for(duration).fuse() => Err(Error::Timeout),
        rsp = f.fuse() => rsp,
    }
}

mod time {
    use std::time::Duration;
    use std::time::Instant;

    use futures_timer::Delay;
    pub struct Interval {
        timer: Option<Delay>,
        interval: Duration,
    }

    impl Interval {
        pub async fn tick(&mut self) -> Instant {
            let timer = self.timer.take().unwrap();
            timer.await;
            let now = Instant::now();
            self.timer = Some(Delay::new(self.interval));
            now
        }
    }

    /// Creates new Interval that yields with interval of duration.
    pub fn interval(duration: Duration) -> Interval {
        Interval {
            timer: Some(Delay::new(Duration::from_secs(0))),
            interval: duration,
        }
    }

    /// Wait until duration has elapsed.
    pub fn delay_for(duration: Duration) -> Delay {
        Delay::new(duration)
    }

    /// Wait until deadline is reached.
    pub fn delay_until(deadline: Instant) -> Delay {
        let mut delay = Delay::new(Duration::from_secs(1));
        delay.reset(deadline);
        delay
    }
}
