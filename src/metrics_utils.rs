use std::time::{Duration, Instant};

use metrics::{describe_counter, describe_gauge, describe_histogram, gauge, histogram};

pub const OPENED_TOTAL: &str = "mobc_pool_connections_opened_total";
pub const CLOSED_TOTAL: &str = "mobc_pool_connections_closed_total";
pub const OPEN_CONNECTIONS: &str = "mobc_pool_connections_open";

pub const ACTIVE_CONNECTIONS: &str = "mobc_pool_connections_busy";
pub const IDLE_CONNECTIONS: &str = "mobc_pool_connections_idle";
pub const WAIT_COUNT: &str = "mobc_client_queries_wait";
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
}

impl GaugeGuard {
    pub fn increment(key: &'static str) -> Self {
        gauge!(key).increment(1.0);
        Self { key }
    }
}

impl Drop for GaugeGuard {
    fn drop(&mut self) {
        gauge!(self.key).decrement(1.0);
    }
}

pub(crate) struct DurationHistogramGuard {
    start: Option<Instant>,
    key: &'static str,
}

impl DurationHistogramGuard {
    pub(crate) fn start(key: &'static str) -> Self {
        Self {
            start: Some(Instant::now()),
            key,
        }
    }

    pub(crate) fn into_elapsed(mut self) -> Duration {
        let start = self
            .start
            .take()
            .expect("start time was unset without consuming or dropping the guard");
        let elapsed = start.elapsed();
        histogram!(self.key).record(elapsed);
        elapsed
    }
}

impl Drop for DurationHistogramGuard {
    fn drop(&mut self) {
        if let Some(start) = self.start.take() {
            histogram!(self.key).record(start.elapsed());
        }
    }
}
