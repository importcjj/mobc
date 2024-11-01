use std::{
    mem::ManuallyDrop,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use metrics::{describe_counter, describe_gauge, describe_histogram, gauge, histogram};

// counters
pub const OPENED_TOTAL: &str = "mobc_pool_connections_opened_total";
pub const CLOSED_TOTAL: &str = "mobc_pool_connections_closed_total";

// gauges
pub const OPEN_CONNECTIONS: &str = "mobc_pool_connections_open";
pub const ACTIVE_CONNECTIONS: &str = "mobc_pool_connections_busy";
pub const IDLE_CONNECTIONS: &str = "mobc_pool_connections_idle";
pub const WAIT_COUNT: &str = "mobc_client_queries_wait";

// histogram
pub const WAIT_DURATION: &str = "mobc_client_queries_wait_histogram_ms";

pub fn describe_metrics() {
    describe_counter!(OPENED_TOTAL, "Total number of Pool Connections opened");
    describe_counter!(CLOSED_TOTAL, "Total number of Pool Connections closed");

    describe_gauge!(
        OPEN_CONNECTIONS,
        "Number of currently open Pool Connections"
    );

    describe_gauge!(
        ACTIVE_CONNECTIONS,
        "Number of currently busy Pool Connections (executing a database query)"
    );

    describe_gauge!(
        IDLE_CONNECTIONS,
        "Number of currently unused Pool Connections (waiting for the next pool query to run)"
    );

    describe_gauge!(
        WAIT_COUNT,
        "Number of queries currently waiting for a connection"
    );

    describe_histogram!(
        WAIT_DURATION,
        "Histogram of the wait time of all queries in ms"
    );
}

pub(crate) struct GaugeGuard {
    key: &'static str,
    already_decremented: AtomicBool,
}

impl GaugeGuard {
    pub fn increment(key: &'static str) -> Self {
        gauge!(key).increment(1.0);
        Self {
            key,
            already_decremented: AtomicBool::new(false),
        }
    }

    pub fn decrement_now(&self) {
        if !self.already_decremented.swap(true, Ordering::SeqCst) {
            gauge!(self.key).decrement(1.0);
        }
    }
}

impl Drop for GaugeGuard {
    fn drop(&mut self) {
        if !self.already_decremented.load(Ordering::SeqCst) {
            gauge!(self.key).decrement(1.0);
        }
    }
}

pub(crate) struct DurationHistogramGuard {
    start: Instant,
    key: &'static str,
}

impl DurationHistogramGuard {
    pub(crate) fn start(key: &'static str) -> Self {
        Self {
            start: Instant::now(),
            key,
        }
    }

    pub(crate) fn into_elapsed(self) -> Duration {
        let this = ManuallyDrop::new(self);
        let elapsed = this.start.elapsed();
        histogram!(this.key).record(elapsed);
        elapsed
    }
}

impl Drop for DurationHistogramGuard {
    fn drop(&mut self) {
        histogram!(self.key).record(self.start.elapsed());
    }
}
