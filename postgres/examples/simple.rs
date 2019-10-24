use mobc::Error;
use mobc::Pool;
use mobc_postgres::tokio_postgres;
use mobc_postgres::PostgresConnectionManager;
use std::str::FromStr;
use tokio_postgres::Config;
use tokio_postgres::Error as PostgresError;
use tokio_postgres::NoTls;

async fn do_postgres() -> Result<(), Error<PostgresError>> {
    let config = Config::from_str("postgres://jiaju:jiaju@localhost:5432")?;
    let manager = PostgresConnectionManager::new(config, NoTls);
    let pool = Pool::new(manager).await?;

    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = do_postgres().await {
        println!("some error");
    }
    loop {}
}
