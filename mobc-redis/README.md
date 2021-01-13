# mobc-redis

[![crates.io](https://img.shields.io/badge/crates.io-latest-%23dea584)](https://crates.io/crates/mobc-redis)

[Documentation](https://docs.rs/mobc-redis)

## Example 

```rust
use mobc::Pool;
use mobc_redis::redis;
use redis::AsyncCommands;
use mobc_redis::RedisConnectionManager;
use std::time::Instant;

const TEST_KEY: &'static str = "mobc::redis::test";

#[tokio::main]
async fn main() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let manager = RedisConnectionManager::new(client);
    let pool = Pool::builder().max_open(20).build(manager);

    let mut conn = pool.get().await.unwrap();
    let _: () = conn.set(TEST_KEY, "hello").await.unwrap();

    const MAX: usize = 5000;

    let now = Instant::now();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<usize>(5000);
    for i in 0..MAX {
        let pool = pool.clone();
        let tx_c = tx.clone();
        tokio::spawn(async move {
            let mut conn = pool.get().await.unwrap();
            let s: String = conn.get(TEST_KEY).await.unwrap();
            assert_eq!(s.as_str(), "hello");
            tx_c.send(i).await.unwrap();
        });
    }
    for _ in 0..MAX {
        rx.recv().await.unwrap();
    }

    println!("cost: {:?}", now.elapsed());
}

```

## Runtimes

You can use either [tokio](https://github.com/tokio-rs/tokio), or [async-std](https://github.com/async-rs/async-std) as your async runtime.

The default is `tokio-comp`, but you can use `async-std` by setting it in the `mobc-redis` features.

```mobc-redis = { version = "0.7", default-features = false, features = ["async-std-comp"] }```
