use mobc2::{async_trait, Manager};
use tide::Request;

use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Debug)]
pub struct FooError;

pub struct FooConnection(i64);

impl FooConnection {
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    pub async fn query(&self) -> String {
        format!("Hello from connection<{}>", self.0)
    }
}

pub struct FooManager {
    seq_id: AtomicI64,
}

impl FooManager {
    pub fn new() -> Self {
        Self {
            seq_id: AtomicI64::new(0),
        }
    }
}

#[async_trait]
impl Manager for FooManager {
    type Connection = FooConnection;
    type Error = FooError;
    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        self.seq_id.fetch_add(1, Ordering::Relaxed);
        Ok(FooConnection::new(self.seq_id.load(Ordering::Relaxed)))
    }
}

type Pool = mobc2::Pool<FooManager>;

async fn ping(req: Request<Pool>) -> tide::Result {
    let pool = req.state();
    let conn = pool.get().await.unwrap();
    Ok(conn.query().await.into())
}

async fn ping2(_: Request<Pool>) -> tide::Result {
    Ok("Hello from foo connection".into())
}

#[async_std::main]
async fn main() {
    let manager = FooManager::new();
    let pool = mobc2::Pool::new(manager, 500);

    let mut app = tide::with_state(pool);
    app.at("/ping").get(ping);
    app.at("/ping2").get(ping2);
    app.listen("0.0.0.0:7777").await.unwrap();
}
