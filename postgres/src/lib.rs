use mobc::futures::{FutureExt, TryFutureExt};
use mobc::AnyFuture;
use mobc::ConnectionManager;
use tokio_executor::DefaultExecutor;
use tokio_executor::Executor;
pub use tokio_postgres;
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres::Error;
use tokio_postgres::Socket;

pub struct PostgresConnectionManager<Tls> {
    config: Config,
    tls: Tls,
}

impl<Tls> PostgresConnectionManager<Tls> {
    pub fn new(config: Config, tls: Tls) -> Self {
        PostgresConnectionManager {
            config: config,
            tls: tls,
        }
    }
}

impl<Tls> ConnectionManager for PostgresConnectionManager<Tls>
where
    Tls: MakeTlsConnect<Socket> + Clone + Send + Sync + 'static,
    <Tls as MakeTlsConnect<Socket>>::Stream: Send + Sync,
    <Tls as MakeTlsConnect<Socket>>::TlsConnect: Send,
    <<Tls as MakeTlsConnect<Socket>>::TlsConnect as TlsConnect<Socket>>::Future: Send,
{
    type Connection = Client;
    type Executor = DefaultExecutor;
    type Error = Error;

    fn get_executor(&self) -> Self::Executor {
        DefaultExecutor::current()
    }

    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
        let mut executor = self.get_executor().clone();
        let config = self.config.clone();
        let tls = self.tls.clone();
        let connect_fut = async move { config.connect(tls).await };
        Box::new(Box::pin(connect_fut.map_ok(move |(client, conn)| {
            executor.spawn(Box::pin(conn.map(|_| ())));
            client
        })))
    }

    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
        let simple_query_fut = async move {
            conn.execute("", &[]).await?;
            Ok(conn)
        };
        Box::new(Box::pin(simple_query_fut))
    }

    fn has_broken(&self, conn: &mut Option<Self::Connection>) -> bool {
        match conn {
            Some(ref raw) => raw.is_closed(),
            None => false,
        }
    }
}
