use mobc::Pool;
use std::str::FromStr;
use std::time::Instant;
use mobc_postgres::PgConnectionManager;
use tokio_postgres::Config;
use tokio_postgres::NoTls;


#[tokio::main]
async fn main() {
    let config = Config::from_str("postgres://jiaju:jiaju@localhost:5432").unwrap();
    let manager = PgConnectionManager::new(config, NoTls);
    let pool = Pool::builder().max_open(20).build(manager);
    const MAX: usize = 5000;

    let now = Instant::now();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<usize>(16);
    for i in 0..MAX {
        let pool = pool.clone();
        let tx_c = tx.clone();
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