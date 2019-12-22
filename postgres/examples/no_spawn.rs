use mobc::Pool;
use mobc_postgres::tokio_postgres;
use mobc_postgres::PostgresConnectionManager;
use std::str::FromStr;
use std::time::Instant;
use tokio_postgres::Config;
use tokio_postgres::NoTls;
#[tokio::main]
async fn main() {
    let mark = Instant::now();
    let config = Config::from_str("postgres://jiaju:jiaju@localhost:5432").unwrap();
    let manager = PostgresConnectionManager::new(config, NoTls);
    let pool = Pool::new(manager).await.unwrap();

    for i in 0..5000usize {
        let mut client = pool.get().await.unwrap();
        let r = client.execute("SELECT 1", &[]).await.unwrap();
        assert_eq!(r, 1);
    }

    println!("cost {:?}", mark.elapsed());
}
