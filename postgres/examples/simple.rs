use mobc::Error;
use mobc::Pool;
use mobc::{Future01CompatExt, FutureExt};
use mobc_postgres::tokio_postgres;
use mobc_postgres::PostgresConnectionManager;
use std::str::FromStr;
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres::Error as PostgresError;
use tokio_postgres::NoTls;

const MAX: usize = 1000;

async fn do_postgres() -> Result<(), Error<PostgresError>> {
    let config = Config::from_str("postgres://jiaju:jiaju@localhost:5432")?;
    let manager = PostgresConnectionManager::new(config, NoTls);
    let pool = Pool::new(manager).await?;

    async fn simple_query(
        pool: Pool<PostgresConnectionManager<NoTls>>,
    ) -> Result<(), Error<PostgresError>> {
        let conn = pool.get().await?;
        let statement = conn.prepare("SELECT 1").await?;
        let r = conn.execute(&statement, &[]).await?;
        println!("query result! {:?}", r);
        Ok(())
    }

    for _ in 0..MAX {
        let pool = pool.clone();
        tokio::spawn(simple_query(pool).map(|_| ()));
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = do_postgres().await {
        println!("some error");
    }
    loop {}
}
