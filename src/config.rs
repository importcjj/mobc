use crate::metrics_utils::describe_metrics;
use crate::{Manager, Pool};
use std::marker::PhantomData;
use std::time::Duration;

const DEFAULT_MAX_IDLE_CONNS: u64 = 2;
const DEFAULT_MAX_OPEN_CONNS: u64 = 10;
const DEFAULT_BAD_CONN_RETRIES: u32 = 2;

pub(crate) struct Config {
    pub max_open: u64,
    pub max_idle: u64,
    pub max_lifetime: Option<Duration>,
    pub max_idle_lifetime: Option<Duration>,
    pub clean_rate: Duration,
    pub max_bad_conn_retries: u32,
    pub get_timeout: Option<Duration>,
    pub health_check_interval: Option<Duration>,
    pub health_check: bool,
}

impl Config {
    pub fn split(self) -> (ShareConfig, InternalConfig) {
        let share = ShareConfig {
            clean_rate: self.clean_rate,
            max_bad_conn_retries: self.max_bad_conn_retries,
            get_timeout: self.get_timeout,
            health_check: self.health_check,
            health_check_interval: self.health_check_interval,
        };

        let internal = InternalConfig {
            max_open: self.max_open,
            max_idle: self.max_idle,
            max_lifetime: self.max_lifetime,
            max_idle_lifetime: self.max_idle_lifetime,
        };

        (share, internal)
    }
}

pub(crate) struct ShareConfig {
    pub clean_rate: Duration,
    pub max_bad_conn_retries: u32,
    pub get_timeout: Option<Duration>,
    pub health_check: bool,
    pub health_check_interval: Option<Duration>,
}

#[derive(Clone)]
pub(crate) struct InternalConfig {
    pub max_open: u64,
    pub max_idle: u64,
    pub max_lifetime: Option<Duration>,
    pub max_idle_lifetime: Option<Duration>,
}

/// A builder for a connection pool.
pub struct Builder<M> {
    max_open: u64,
    max_idle: Option<u64>,
    max_lifetime: Option<Duration>,
    max_idle_lifetime: Option<Duration>,
    clean_rate: Duration,
    max_bad_conn_retries: u32,
    get_timeout: Option<Duration>,
    health_check_interval: Option<Duration>,
    health_check: bool,
    _keep: PhantomData<M>,
}

impl<M> Default for Builder<M> {
    fn default() -> Self {
        Self {
            max_open: DEFAULT_MAX_OPEN_CONNS,
            max_idle: None,
            max_lifetime: None,
            max_idle_lifetime: None,
            clean_rate: Duration::from_secs(1),
            max_bad_conn_retries: DEFAULT_BAD_CONN_RETRIES,
            get_timeout: Some(Duration::from_secs(30)),
            _keep: PhantomData,
            health_check: true,
            health_check_interval: None,
        }
    }
}

impl<M: Manager> Builder<M> {
    /// Constructs a new `Builder`.
    ///
    /// Parameters are initialized with their default values.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the maximum number of connections managed by the pool.
    ///
    /// - 0 means unlimited.
    /// - Defaults to 10.
    pub fn max_open(mut self, max_open: u64) -> Self {
        self.max_open = max_open;
        self
    }

    /// Sets the maximum idle connection count maintained by the pool.
    ///
    /// The pool will maintain at most this many idle connections
    /// at all times, while respecting the value of `max_open`.
    ///
    /// - 0 means unlimited (limited only by `max_open`).
    /// - Defaults to 2.
    pub fn max_idle(mut self, max_idle: u64) -> Self {
        self.max_idle = Some(max_idle);
        self
    }

    /// If true, the health of a connection will be verified via a call to
    /// `Manager::check` before it is checked out of the pool.
    ///
    /// - Defaults to true.
    pub fn test_on_check_out(mut self, health_check: bool) -> Builder<M> {
        self.health_check = health_check;
        self
    }

    /// Sets the maximum lifetime of connections in the pool.
    ///
    /// Expired connections may be closed lazily before reuse.
    ///
    /// - `None` means reuse forever.
    /// - Defaults to `None`.
    ///
    /// # Panics
    ///
    /// Panics if `max_lifetime` is the zero `Duration`.
    pub fn max_lifetime(mut self, max_lifetime: Option<Duration>) -> Self {
        assert_ne!(
            max_lifetime,
            Some(Duration::from_secs(0)),
            "max_lifetime must be positive"
        );
        self.max_lifetime = max_lifetime;
        self
    }

    /// Sets the maximum lifetime of connection to be idle in the pool,
    /// resetting the timer when connection is used.
    ///
    /// Expired connections may be closed lazily before reuse.
    ///
    /// - `None` means reuse forever.
    /// - Defaults to `None`.
    ///
    /// # Panics
    ///
    /// Panics if `max_idle_lifetime` is the zero `Duration`.
    pub fn max_idle_lifetime(mut self, max_idle_lifetime: Option<Duration>) -> Self {
        assert_ne!(
            max_idle_lifetime,
            Some(Duration::from_secs(0)),
            "max_idle_lifetime must be positive"
        );
        self.max_idle_lifetime = max_idle_lifetime;
        self
    }

    /// Sets the get timeout used by the pool.
    ///
    /// Calls to `Pool::get` will wait this long for a connection to become
    /// available before returning an error.
    ///
    /// - `None` means never timeout.
    /// - Defaults to 30 seconds.
    ///
    /// # Panics
    ///
    /// Panics if `connection_timeout` is the zero duration
    pub fn get_timeout(mut self, get_timeout: Option<Duration>) -> Self {
        assert_ne!(
            get_timeout,
            Some(Duration::from_secs(0)),
            "get_timeout must be positive"
        );

        self.get_timeout = get_timeout;
        self
    }

    /// Sets the interval how often a connection will be checked when returning
    /// an existing connection from the pool. If set to `None`, a connection is
    /// checked every time when returning from the pool. Must be used together
    /// with [`test_on_check_out`] set to `true`, otherwise does nothing.
    ///
    /// - `None` means a connection is checked every time when returning from the
    ///   pool.
    /// - Defaults to `None`.
    ///
    /// # Panics
    ///
    /// Panics if `connection_timeout` is the zero duration
    ///
    /// [`test_on_check_out`]: #method.test_on_check_out
    pub fn health_check_interval(mut self, health_check_interval: Option<Duration>) -> Self {
        assert_ne!(
            health_check_interval,
            Some(Duration::from_secs(0)),
            "health_check_interval must be positive"
        );

        self.health_check_interval = health_check_interval;
        self
    }
    // used by tests
    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn clean_rate(mut self, clean_rate: Duration) -> Builder<M> {
        assert!(
            clean_rate > Duration::from_secs(0),
            "connection_timeout must be positive"
        );

        if clean_rate > Duration::from_secs(1) {
            self.clean_rate = clean_rate;
        }

        self
    }

    /// Consumes the builder, returning a new, initialized pool.
    pub fn build(self, manager: M) -> Pool<M> {
        describe_metrics();
        let mut max_idle = self.max_idle.unwrap_or(DEFAULT_MAX_IDLE_CONNS);
        if self.max_open > 0 && max_idle > self.max_open {
            max_idle = self.max_open
        };

        let config = Config {
            max_open: self.max_open,
            max_idle,
            max_lifetime: self.max_lifetime,
            max_idle_lifetime: self.max_idle_lifetime,
            get_timeout: self.get_timeout,
            clean_rate: self.clean_rate,
            max_bad_conn_retries: self.max_bad_conn_retries,
            health_check: self.health_check,
            health_check_interval: self.health_check_interval,
        };

        Pool::new_inner(manager, config)
    }
}
