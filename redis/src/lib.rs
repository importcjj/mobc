use futures_01::Future;
use mobc::futures::{compat::Future01CompatExt, TryFutureExt};
use mobc::AnyFuture;
use mobc::ConnectionManager;
pub use redis;
use redis::aio::{Connection, ConnectionLike};
use redis::Client;
use redis::RedisError;
use redis::RedisFuture;
use redis::Value;
use std::sync::Arc;

pub struct RedisConnectionManager {
    client: Client,
}

impl RedisConnectionManager {
    pub fn new(client: Client) -> Self {
        RedisConnectionManager { client }
    }
}

impl ConnectionManager for RedisConnectionManager {
    type Connection = Connection;
    type Error = redis::RedisError;

    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
        Box::new(self.client.get_async_connection().compat())
    }

    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
        Box::new(
            redis::cmd("PING")
                .query_async::<_, String>(conn)
                .compat()
                .map_ok(|r| r.0),
        )
    }

    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        false
    }
}
