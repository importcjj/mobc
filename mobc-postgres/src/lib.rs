pub use tokio_postgres;
pub use mobc;
use mobc::Manager;
use mobc::async_trait;
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres::Error;
use tokio_postgres::Socket;


/// An `mobc::Manager` for `tokio_postgres::Client`s.
///
/// ## Example
///
/// ```no_run
/// use mobc::Pool;
/// use std::str::FromStr;
/// use std::time::Instant;
/// use mobc_postgres::PgConnectionManager;
/// use tokio_postgres::Config;
/// use tokio_postgres::NoTls;
///
/// #[tokio::main]
/// async fn main() {
///     let config = Config::from_str("postgres://jiaju:jiaju@localhost:5432").unwrap();
///     let manager = PgConnectionManager::new(config, NoTls);
///     let pool = Pool::builder().max_open(20).build(manager);
///     const MAX: usize = 5000;
///
///     let now = Instant::now();
///     let (tx, mut rx) = tokio::sync::mpsc::channel::<usize>(16);
///     for i in 0..MAX {
///         let pool = pool.clone();
///         let mut tx_c = tx.clone();
///         tokio::spawn(async move {
///             let client = pool.get().await.unwrap();
///             let rows = client.query("SELECT 1 + 2", &[]).await.unwrap();
///             let value: i32 = rows[0].get(0);
///             assert_eq!(value, 3);
///             tx_c.send(i).await.unwrap();
///         });
///     }
///     for _ in 0..MAX {
///         rx.recv().await.unwrap();
///     }
///
///     println!("cost: {:?}", now.elapsed());
/// }
/// ```
pub struct PgConnectionManager<Tls> {
    config: Config,
    tls: Tls,
}

impl<Tls> PgConnectionManager<Tls> {
    pub fn new(config: Config, tls: Tls) -> Self {
        Self { config, tls }
    }
}

#[async_trait]
impl<Tls> Manager for PgConnectionManager<Tls>
where
    Tls: MakeTlsConnect<Socket> + Clone + Send + Sync + 'static,
    <Tls as MakeTlsConnect<Socket>>::Stream: Send + Sync,
    <Tls as MakeTlsConnect<Socket>>::TlsConnect: Send,
    <<Tls as MakeTlsConnect<Socket>>::TlsConnect as TlsConnect<Socket>>::Future: Send,
{
    type Connection = Client;
    type Error = Error;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let tls = self.tls.clone();
        let (client, conn) = self.config.connect(tls).await?;
        mobc::spawn(conn);
        Ok(client)
    }

    async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
        conn.simple_query("").await?;
        Ok(conn)
    }
}