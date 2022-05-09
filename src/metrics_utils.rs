use metrics::{describe_gauge, describe_histogram};

pub const ACTIVE_CONNECTIONS: &str = "pool_active_connections";
pub const IDLE_CONNECTIONS: &str = "pool_idle_connections";
pub const WAIT_COUNT: &str = "pool_wait_count";
pub const WAIT_DURATION: &str = "pool_wait_duration";

pub fn describe_metrics() {
    describe_gauge!(
        ACTIVE_CONNECTIONS,
        "The number of active connections in use."
    );

    describe_gauge!(
        IDLE_CONNECTIONS,
        "The number of connections that are not being used"
    );

    describe_gauge!(
        WAIT_COUNT,
        "The number of workers waiting for a connection."
    );

    describe_histogram!(
        WAIT_DURATION,
        "The wait time for a worker to get a connection."
    );
}
