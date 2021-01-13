pub use redis;
pub use mobc;

use mobc::async_trait;
use mobc::Manager;
use redis::aio::Connection;
use redis::{Client, ErrorKind};

pub struct RedisConnectionManager {
    client: Client,
}

impl RedisConnectionManager {
    pub fn new(c: Client) -> Self {
        Self { client: c }
    }
}

#[async_trait]
impl Manager for RedisConnectionManager {
    type Connection = Connection;
    type Error = redis::RedisError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let c = self.client.get_async_connection().await?;
        Ok(c)
    }

    async fn check(&self, mut conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
        let pong: String = redis::cmd("PING").query_async(&mut conn).await?;
        if pong.as_str() != "PONG" {
            return Err((ErrorKind::ResponseError, "pong response error").into());
        }
        Ok(conn)
    }
}


