use mobc::Error;
use mobc::Future01CompatExt;
use mobc::Pool;
use mobc_redis::redis::{self, RedisError};
use mobc_redis::RedisConnectionManager;
use std::time::Instant;
use tokio::executor::DefaultExecutor;
use tokio::prelude::*;
use tokio::sync::mpsc;

const MAX: usize = 10000;

async fn ping(
    pool: Pool<RedisConnectionManager<DefaultExecutor>>,
    mut sender: mpsc::Sender<()>,
) -> Result<(), Error<RedisError>> {
    let mut conn = pool.get().await?;
    let raw_conn = conn.take_raw_conn();

    let (raw_conn, pong) = redis::cmd("PING")
        .query_async::<_, String>(raw_conn)
        .compat()
        .await?;

    conn.set_raw_conn(raw_conn);

    assert_eq!("PONG", pong);
    sender.send(()).await.unwrap();
    Ok(())
}

async fn do_redis(sender: mpsc::Sender<()>) -> Result<(), Error<RedisError>> {
    let client = redis::Client::open("redis://127.0.0.1").unwrap();
    let manager = RedisConnectionManager::new(client);
    let pool = Pool::builder().max_size(40).build(manager).await?;

    println!("pool was created");

    for _ in 0..MAX {
        let pool = pool.clone();
        let tx = sender.clone();
        tokio::spawn(ping(pool, tx).map(|_| ()));
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let mark = Instant::now();
    let (tx, mut rx) = mpsc::channel::<()>(MAX);
    if let Err(_) = do_redis(tx).await {
        println!("some error");
    }
    let mut num: usize = 0;
    while let Some(_) = rx.next().await {
        num += 1;
        if num == MAX {
            break;
        }
    }

    println!("cost {:?}", mark.elapsed());
}
