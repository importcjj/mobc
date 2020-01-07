use mobc::Pool;
use mobc_redis::RedisConnectionManager;
use mobc_redis::{redis, Connection};
use std::time::Instant;

#[tokio::main]
async fn main() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let manager = RedisConnectionManager::new(client);
    let pool = Pool::builder().max_open(100).build(manager);

    const MAX: usize = 5000;

    let now = Instant::now();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<usize>(16);
    for i in 0..MAX {
        let pool = pool.clone();
        let mut tx_c = tx.clone();
        tokio::spawn(async move {
            let mut conn = pool.get().await.unwrap();
            let s: String = redis::cmd("PING")
                .query_async(&mut conn as &mut Connection)
                .await
                .unwrap();
            assert_eq!(s.as_str(), "PONG");
            tx_c.send(i).await.unwrap();
        });
    }
    for _ in 0..MAX {
        rx.recv().await.unwrap();
    }

    println!("cost: {:?}", now.elapsed());
}
