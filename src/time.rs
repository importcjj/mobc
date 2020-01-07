pub use time::{delay_for, delay_until, interval};

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

    pub fn interval(duration: Duration) -> Interval {
        Interval {
            timer: Some(Delay::new(Duration::from_secs(0))),
            interval: duration,
        }
    }

    pub fn delay_for(duration: Duration) -> Delay {
        Delay::new(duration)
    }

    pub fn delay_until(deadline: Instant) -> Delay {
        let mut delay = Delay::new(Duration::from_secs(1));
        delay.reset(deadline);
        delay
    }
}
