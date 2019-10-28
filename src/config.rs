use crate::ConnectionManager;
use crate::Error;
use crate::Pool;
use std::marker::PhantomData;
use std::time::Duration;

pub struct Config<T> {
    pub max_size: u32,
    pub min_idle: Option<u32>,
    pub max_concurrency: u32,
    pub executor: T,
    pub connection_timeout: Duration,
    pub max_lifetime: Option<Duration>,
    pub idle_timeout: Option<Duration>,
}

pub struct Builder<M>
where
    M: ConnectionManager,
{
    max_size: u32,
    min_idle: Option<u32>,
    max_concurrency: u32,
    max_lifetime: Option<Duration>,
    idle_timeout: Option<Duration>,
    connection_timeout: Duration,
    reaper_rate: Duration,
    _keep: PhantomData<M>,
}

impl<M> Default for Builder<M>
where
    M: ConnectionManager,
{
    fn default() -> Self {
        Self {
            max_size: 10,
            min_idle: None,
            max_concurrency: 10000,
            idle_timeout: Some(Duration::from_secs(10 * 60)),
            max_lifetime: Some(Duration::from_secs(30 * 60)),
            connection_timeout: Duration::from_secs(30),
            reaper_rate: Duration::from_secs(30),
            _keep: PhantomData,
        }
    }
}

impl<M> Builder<M>
where
    M: ConnectionManager,
{
    /// Constructs a new `Builder`.
    ///
    /// Parameters are initialized with their default values.
    pub fn new() -> Self {
        Builder::default()
    }

    /// Sets the maximum number of connections managed by the pool.
    ///
    /// Defaults to 10.
    ///
    /// # Panics
    ///
    /// Panics if `max_size` is 0.
    pub fn max_size(mut self, max_size: u32) -> Self {
        assert!(max_size > 0, "max_size must be positive");
        self.max_size = max_size;
        self
    }

    pub fn max_concurrency(mut self, max_concurrency: u32) -> Self {
        assert!(max_concurrency > 0, "max_concurrency must be positive");
        self.max_concurrency = max_concurrency;
        self
    }

    /// Sets the minimum idle connection count maintained by the pool.
    ///
    /// If set, the pool will try to maintain at least this many idle
    /// connections at all times, while respecting the value of `max_size`.
    ///
    /// Defaults to `None` (equivalent to the value of `max_size`).
    pub fn min_idle(mut self, min_idle: Option<u32>) -> Self {
        self.min_idle = min_idle;
        self
    }

    /// Sets the maximum lifetime of connections in the pool.
    ///
    /// If set, connections will be closed after existing for at most 30 seconds
    /// beyond this duration.
    ///
    /// If a connection reaches its maximum lifetime while checked out it will
    /// be closed when it is returned to the pool.
    ///
    /// Defaults to 30 minutes.
    ///
    /// # Panics
    ///
    /// Panics if `max_lifetime` is the zero `Duration`.
    pub fn max_lifetime(mut self, max_lifetime: Option<Duration>) -> Builder<M> {
        assert_ne!(
            max_lifetime,
            Some(Duration::from_secs(0)),
            "max_lifetime must be positive"
        );
        self.max_lifetime = max_lifetime;
        self
    }

    /// Sets the idle timeout used by the pool.
    ///
    /// If set, connections will be closed after sitting idle for at most 30
    /// seconds beyond this duration.
    ///
    /// Defaults to 10 minutes.
    ///
    /// # Panics
    ///
    /// Panics if `idle_timeout` is the zero `Duration`.
    pub fn idle_timeout(mut self, idle_timeout: Option<Duration>) -> Builder<M> {
        assert_ne!(
            idle_timeout,
            Some(Duration::from_secs(0)),
            "idle_timeout must be positive"
        );
        self.idle_timeout = idle_timeout;
        self
    }

    /// Sets the connection timeout used by the pool.
    ///
    /// Calls to `Pool::get` will wait this long for a connection to become
    /// available before returning an error.
    ///
    /// Defaults to 30 seconds.
    ///
    /// # Panics
    ///
    /// Panics if `connection_timeout` is the zero duration
    pub fn connection_timeout(mut self, connection_timeout: Duration) -> Builder<M> {
        assert!(
            connection_timeout > Duration::from_secs(0),
            "connection_timeout must be positive"
        );
        self.connection_timeout = connection_timeout;
        self
    }

    /// Consumes the builder, returning a new, initialized pool.
    ///
    /// It will block until the pool has established its configured minimum
    /// number of connections, or it times out.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool is unable to open its minimum number of
    /// connections.
    ///
    /// # Panics
    ///
    /// Panics if `min_idle` is greater than `max_size`.
    pub async fn build<E>(self, manager: M) -> Result<Pool<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        let pool = self.build_unchecked(manager).await;
        pool.wait_for_initialization().await?;
        Ok(pool)
    }

    /// Consumes the builder, returning a new pool.
    ///
    /// Unlike `build`, this method does not wait for any connections to be
    /// established before returning.
    ///
    /// # Panics
    ///
    /// Panics if `min_idle` is greater than `max_size`.
    pub async fn build_unchecked(self, manager: M) -> Pool<M> {
        if let Some(min_idle) = self.min_idle {
            assert!(
                self.max_size >= min_idle,
                "min_idle must be no larger than max_size"
            );
        }

        let config = Config {
            max_size: self.max_size,
            min_idle: self.min_idle,
            max_lifetime: self.max_lifetime,
            idle_timeout: self.idle_timeout,
            max_concurrency: self.max_concurrency,
            connection_timeout: self.connection_timeout,
            executor: manager.get_executor(),
        };

        Pool::new_inner(config, manager, self.reaper_rate).await
    }
}
