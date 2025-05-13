use std::{
    any::type_name,
    marker::PhantomData,
    mem::ManuallyDrop,
    time::{Duration, Instant},
};

use metrics::{describe_counter, describe_gauge, describe_histogram, gauge, histogram};

use crate::Manager;

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

pub(crate) fn get_manager_type<M: Manager>() -> &'static str {
    type_name::<M>()
        .split("::")
        .last()
        .expect("we shouldn't get None here")
}

pub(crate) struct GaugeGuard<M: Manager> {
    key: &'static str,
    _phantom: PhantomData<M>,
}

impl<M: Manager> GaugeGuard<M> {
    pub fn increment(key: &'static str) -> Self {
        gauge!(key, "manager" => get_manager_type::<M>()).increment(1.0);
        Self {
            key,
            _phantom: PhantomData,
        }
    }
}

impl<M: Manager> Drop for GaugeGuard<M> {
    fn drop(&mut self) {
        gauge!(self.key, "manager" => get_manager_type::<M>()).decrement(1.0);
    }
}

pub(crate) struct DurationHistogramGuard<M: Manager> {
    start: Instant,
    key: &'static str,
    _phantom: PhantomData<M>,
}

impl<M: Manager> DurationHistogramGuard<M> {
    pub(crate) fn start(key: &'static str) -> Self {
        Self {
            start: Instant::now(),
            key,
            _phantom: PhantomData,
        }
    }

    pub(crate) fn into_elapsed(self) -> Duration {
        let this = ManuallyDrop::new(self);
        let elapsed = this.start.elapsed();
        histogram!(this.key, "manager" => get_manager_type::<M>()).record(elapsed);
        elapsed
    }
}

impl<M: Manager> Drop for DurationHistogramGuard<M> {
    fn drop(&mut self) {
        histogram!(self.key).record(self.start.elapsed());
    }
}
