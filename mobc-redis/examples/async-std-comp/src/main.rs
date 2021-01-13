use mobc_redis::mobc::Pool;
use mobc_redis::redis;
use redis::AsyncCommands;
use mobc_redis::RedisConnectionManager;
use std::time::Instant;
use async_std::task;

const test_key: &'static str = "mobc::redis::test";

#[async_std::main]
async fn main() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let manager = RedisConnectionManager::new(client);
    let pool = Pool::builder().max_open(20).build(manager);

    let mut conn = pool.get().await.unwrap();
    let _: () = conn.set(test_key, "hello").await.unwrap();

    const MAX: usize = 5000;

    let now = Instant::now();
    let (tx, mut rx) = async_channel::bounded::<usize>(5000);
    for i in 0..MAX {
        let pool = pool.clone();
        let tx_c = tx.clone();
        task::spawn(async move {
            let mut conn = pool.get().await.unwrap();
            let s: String = conn.get(test_key).await.unwrap();
            assert_eq!(s.as_str(), "hello");
            tx_c.send(i).await.unwrap();
        });
    }
    for _ in 0..MAX {
        rx.recv().await.unwrap();
    }

    println!("cost: {:?}", now.elapsed());
}
