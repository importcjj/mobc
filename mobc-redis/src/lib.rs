pub use mobc;
pub use redis;

use mobc::async_trait;
use mobc::Manager;
use redis::aio::MultiplexedConnection as Connection;
use redis::{Client, ErrorKind};
use std::sync::atomic::{AtomicU32, Ordering};

pub struct RedisConnectionManager {
    client: Client,
    ping_count: AtomicU32,
}

impl RedisConnectionManager {
    pub fn new(c: Client) -> Self {
        Self {
            client: c,
            ping_count: AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl Manager for RedisConnectionManager {
    type Connection = Connection;
    type Error = redis::RedisError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let c = self.client.get_multiplexed_async_connection().await?;
        Ok(c)
    }

    async fn check(&self, mut conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
        let ping = self.ping_count.fetch_add(1, Ordering::SeqCst);
        let ping = format!("PONG {}", ping);
        let pong: String = redis::cmd("PING").arg(&ping).query_async(&mut conn).await?;
        if pong != ping {
            return Err((ErrorKind::ResponseError, "pong response error").into());
        }
        Ok(conn)
    }
}
