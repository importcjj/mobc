use crate::ConnectionManager;
use crate::Error;
use crate::Pool;
use std::marker::PhantomData;

pub struct Config<T> {
    pub max_size: u32,
    pub min_idle: Option<u32>,
    pub executor: Option<T>,
}

pub struct Builer<M>
where
    M: ConnectionManager,
{
    max_size: u32,
    min_idle: Option<u32>,
    executor: Option<M::Executor>,
    _keep: PhantomData<M>,
}

impl<M> Default for Builer<M>
where
    M: ConnectionManager,
{
    fn default() -> Self {
        Self {
            max_size: 10,
            min_idle: None,
            executor: None,
            _keep: PhantomData,
        }
    }
}

impl<M> Builer<M>
where
    M: ConnectionManager,
{
    pub fn new() -> Self {
        Builer::default()
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

    pub async fn build<E>(self, manager: M) -> Result<Pool<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        let pool = self.build_unchecked(manager);
        pool.wait_for_initialization().await?;
        Ok(pool)
    }

    pub fn build_unchecked(self, manager: M) -> Pool<M> {
        if let Some(min_idle) = self.min_idle {
            assert!(
                self.max_size >= min_idle,
                "min_idle must be no larger than max_size"
            );
        }

        let config = Config {
            max_size: self.max_size,
            min_idle: self.min_idle,
            executor: manager.get_executor(),
        };

        Pool::new_inner(config, manager)
    }
}
