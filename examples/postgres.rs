use futures::prelude::*;
use mobc::Manager;
use mobc::Pool;
use mobc::ResultFuture;
use std::str::FromStr;
use std::time::Instant;
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres::Error;
use tokio_postgres::NoTls;
use tokio_postgres::Socket;

pub struct ConnectionManager<Tls> {
    config: Config,
    tls: Tls,
}

impl<Tls> ConnectionManager<Tls> {
    pub fn new(config: Config, tls: Tls) -> Self {
        Self { config, tls }
    }
}

impl<Tls> Manager for ConnectionManager<Tls>
where
    Tls: MakeTlsConnect<Socket> + Clone + Send + Sync + 'static,
    <Tls as MakeTlsConnect<Socket>>::Stream: Send + Sync,
    <Tls as MakeTlsConnect<Socket>>::TlsConnect: Send,
    <<Tls as MakeTlsConnect<Socket>>::TlsConnect as TlsConnect<Socket>>::Future: Send,
{
    type Connection = Client;
    type Error = Error;

    fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
        let config = self.config.clone();
        let tls = self.tls.clone();
        let connect_fut = async move { config.connect(tls).await };
        Box::pin(connect_fut.map_ok(move |(client, conn)| {
            tokio::spawn(conn);
            client
        }))
    }

    fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
        let simple_query_fut = async move {
            conn.simple_query("").await?;
            Ok(conn)
        };
        Box::pin(simple_query_fut)
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let config = tokio_postgres::Config::from_str("postgres://jiaju:jiaju@localhost:5432").unwrap();
    let manager = ConnectionManager::new(config, NoTls);
    let pool = Pool::builder().max_open(20).build(manager);
    const MAX: usize = 5000;

    let now = Instant::now();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<usize>(16);
    for i in 0..MAX {
        let pool = pool.clone();
        let mut tx_c = tx.clone();
        tokio::spawn(async move {
            let client = pool.get().await.unwrap();
            let rows = client.query("SELECT 1 + 2", &[]).await.unwrap();
            let value: i32 = rows[0].get(0);
            assert_eq!(value, 3);
            tx_c.send(i).await.unwrap();
        });
    }
    for _ in 0..MAX {
        rx.recv().await.unwrap();
    }
    
    println!("cost: {:?}", now.elapsed());
}
