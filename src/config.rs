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
    pub clean_rate: Duration,
    pub max_bad_conn_retries: u32,
    pub get_timeout: Duration,
}

/// A builder for a connection pool.
pub struct Builder<M> {
    max_open: u64,
    max_idle: Option<u64>,
    max_lifetime: Option<Duration>,
    clean_rate: Duration,
    max_bad_conn_retries: u32,
    get_timeout: Duration,
    _keep: PhantomData<M>,
}

impl<M> Default for Builder<M> {
    fn default() -> Self {
        Self {
            max_open: DEFAULT_MAX_OPEN_CONNS,
            max_idle: None,
            max_lifetime: None,
            clean_rate: Duration::from_secs(1),
            max_bad_conn_retries: DEFAULT_BAD_CONN_RETRIES,
            get_timeout: Duration::from_secs(30),
            _keep: PhantomData,
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
    /// 0 means unlimited, defaults to 10.
    pub fn max_open(mut self, max_open: u64) -> Self {
        self.max_open = max_open;
        self
    }

    /// Sets the maximum idle connection count maintained by the pool.
    ///
    /// The pool will maintain at most this many idle connections
    /// at all times, while respecting the value of `max_open`.
    ///
    /// Defaults to 2.
    pub fn max_idle(mut self, max_idle: Option<u64>) -> Self {
        self.max_idle = max_idle;
        self
    }

    /// Sets the maximum lifetime of connections in the pool.
    ///
    /// If a connection reaches its maximum lifetime while checked out it will
    /// be closed when it is returned to the pool.
    ///
    /// None meas reuse forever.
    /// Defaults to None.
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

    /// Sets the get timeout used by the pool.
    ///
    /// Calls to `Pool::get` will wait this long for a connection to become
    /// available before returning an error.
    ///
    /// Defaults to 30 seconds.
    ///
    /// # Panics
    ///
    /// Panics if `connection_timeout` is the zero duration
    pub fn get_timeout(mut self, get_timeout: Duration) -> Self {
        assert!(
            get_timeout > Duration::from_secs(0),
            "connection_timeout must be positive"
        );
        self.get_timeout = get_timeout;
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
    ///
    /// # Panics
    ///
    /// Panics if `max_idle` is greater than `max_size`.
    pub fn build(self, manager: M) -> Pool<M> {
        let max_idle = self.max_idle.unwrap_or(DEFAULT_MAX_IDLE_CONNS);

        assert!(
            self.max_open >= max_idle,
            "max_idle must be no larger than max_open"
        );

        let config = Config {
            max_open: self.max_open,
            max_idle: max_idle,
            max_lifetime: self.max_lifetime,
            get_timeout: self.get_timeout,
            clean_rate: self.clean_rate,
            max_bad_conn_retries: self.max_bad_conn_retries,
        };

        Pool::new_inner(manager, config)
    }
}
