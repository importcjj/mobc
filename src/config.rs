use crate::ConnectionManager;
use crate::Error;
use crate::Pool;
use std::marker::PhantomData;
use std::time::Duration;

pub struct Config<T> {
    pub max_size: u32,
    pub min_idle: Option<u32>,
    pub executor: T,
    pub connection_timeout: Duration,
}

pub struct Builder<M>
where
    M: ConnectionManager,
{
    max_size: u32,
    min_idle: Option<u32>,
    connection_timeout: Duration,
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
            connection_timeout: Duration::from_secs(30),
            _keep: PhantomData,
        }
    }
}

impl<M> Builder<M>
where
    M: ConnectionManager,
{
    pub fn new() -> Self {
        Builder::default()
    }

    pub fn max_size(mut self, max_size: u32) -> Self {
        assert!(max_size > 0, "max_size must be positive");
        self.max_size = max_size;
        self
    }

    pub fn min_idle(mut self, min_idle: Option<u32>) -> Self {
        self.min_idle = min_idle;
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

    pub async fn build<E>(self, manager: M) -> Result<Pool<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        let pool = self.build_unchecked(manager).await;
        pool.wait_for_initialization().await?;
        Ok(pool)
    }

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
            connection_timeout: self.connection_timeout,
            executor: manager.get_executor(),
        };

        Pool::new_inner(config, manager).await
    }
}
