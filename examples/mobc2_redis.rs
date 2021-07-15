use mobc2::async_trait;
use mobc2::Manager;
use redis::aio::Connection;
use redis::Client;
use std::ops::DerefMut;
use tide::Request;

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
}

pub type Mobc2RedisPool = mobc2::Pool<RedisConnectionManager>;

async fn ping(req: Request<Mobc2RedisPool>) -> tide::Result {
    let pool = req.state();
    let mut conn = pool.get().await.unwrap();
    let res: String = redis::cmd("PING").query_async(conn.deref_mut()).await?;
    Ok(res.into())
}

#[async_std::main]
async fn main() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let manager = RedisConnectionManager::new(client);
    let pool = Mobc2RedisPool::new(manager, 100);

    let mut app = tide::with_state(pool);
    app.at("/mobc2").get(ping);
    app.listen("0.0.0.0:7778").await.unwrap();
}
