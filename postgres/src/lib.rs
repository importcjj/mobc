use mobc::futures::{FutureExt, TryFutureExt};
use mobc::AnyFuture;
use mobc::ConnectionManager;
use mobc::Executor;
use tokio_executor::DefaultExecutor;
pub use tokio_postgres;
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres::Error;
use tokio_postgres::Socket;

pub struct PostgresConnectionManager<Tls, U>
where
    U: Executor + Send + Sync + 'static + Clone,
{
    config: Config,
    tls: Tls,
    executor: U,
}

impl<Tls> PostgresConnectionManager<Tls, DefaultExecutor> {
    pub fn new(config: Config, tls: Tls) -> Self {
        PostgresConnectionManager {
            config,
            tls,
            executor: DefaultExecutor::current(),
        }
    }
}

impl<Tls, U> PostgresConnectionManager<Tls, U>
where
    U: Executor + Send + Sync + 'static + Clone,
{
    pub fn new_with_executor(config: Config, tls: Tls, executor: U) -> Self {
        PostgresConnectionManager {
            config,
            tls,
            executor,
        }
    }
}

impl<Tls, U> ConnectionManager for PostgresConnectionManager<Tls, U>
where
    Tls: MakeTlsConnect<Socket> + Clone + Send + Sync + 'static,
    <Tls as MakeTlsConnect<Socket>>::Stream: Send + Sync,
    <Tls as MakeTlsConnect<Socket>>::TlsConnect: Send,
    <<Tls as MakeTlsConnect<Socket>>::TlsConnect as TlsConnect<Socket>>::Future: Send,
    U: Executor + Send + Sync + 'static + Clone,
{
    type Connection = Client;
    type Executor = U;
    type Error = Error;

    fn get_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
        let mut executor = self.get_executor().clone();
        let config = self.config.clone();
        let tls = self.tls.clone();
        let connect_fut = async move { config.connect(tls).await };
        Box::pin(connect_fut.map_ok(move |(client, conn)| {
            let _ = executor.spawn(Box::pin(conn.map(|_| ())));
            client
        }))
    }

    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
        let simple_query_fut = async move {
            conn.execute("", &[]).await?;
            Ok(conn)
        };
        Box::pin(simple_query_fut)
    }

    fn has_broken(&self, conn: &mut Option<Self::Connection>) -> bool {
        match conn {
            Some(ref raw) => raw.is_closed(),
            None => false,
        }
    }
}
