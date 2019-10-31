pub use mobc;
use mobc::futures::{compat::Future01CompatExt, TryFutureExt};
use mobc::AnyFuture;
use mobc::ConnectionManager;
use mobc::Executor;
pub use redis;
use redis::aio::Connection;
use redis::Client;
use tokio_executor::DefaultExecutor;

pub struct RedisConnectionManager<T>
where
    T: Executor + Send + Sync + 'static + Clone,
{
    client: Client,
    executor: T,
}

impl RedisConnectionManager<DefaultExecutor> {
    pub fn new(client: Client) -> Self {
        RedisConnectionManager {
            client,
            executor: DefaultExecutor::current(),
        }
    }
}

impl<T> RedisConnectionManager<T>
where
    T: Executor + Send + Sync + 'static + Clone,
{
    pub fn new_with_executor(client: Client, executor: T) -> Self {
        RedisConnectionManager { client, executor }
    }
}

impl<T> ConnectionManager for RedisConnectionManager<T>
where
    T: Executor + Send + Sync + 'static + Clone,
{
    type Connection = Connection;
    type Error = redis::RedisError;
    type Executor = T;

    fn get_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
        Box::pin(self.client.get_async_connection().compat())
    }

    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
        Box::pin(
            redis::cmd("PING")
                .query_async::<_, String>(conn)
                .compat()
                .map_ok(|r| r.0),
        )
    }

    fn has_broken(&self, conn: &mut Option<Self::Connection>) -> bool {
        match conn {
            Some(_) => false,
            None => true,
        }
    }
}
