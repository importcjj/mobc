use mobc::futures::channel::mpsc;
use mobc::futures::compat::Future01CompatExt;
use mobc::futures::prelude::*;
use mobc::runtime::Runtime;
use mobc::runtime::TaskExecutor;
use mobc::Error;
use mobc::Executor;
use mobc::Pool;
use mobc_redis::redis::{self, RedisError};
use mobc_redis::RedisConnectionManager;
use std::time::Instant;

const MAX: usize = 5000;

async fn single_request(
    pool: Pool<RedisConnectionManager<TaskExecutor>>,
    mut sender: mpsc::Sender<()>,
) -> Result<(), Error<RedisError>> {
    let mut conn = pool.get().await?;
    let mark = Instant::now();
    let raw_redis_conn = conn.take_raw_conn();

    let (raw_redis_conn, pong) = redis::cmd("PING")
        .query_async::<_, String>(raw_redis_conn)
        .compat()
        .await?;

    conn.set_raw_conn(raw_redis_conn);

    println!("ping costs {:?}", mark.elapsed());

    assert_eq!("PONG", pong);
    sender.send(()).await.unwrap();
    Ok(())
}

async fn do_redis(
    mut executor: TaskExecutor,
    sender: mpsc::Sender<()>,
) -> Result<(), Error<RedisError>> {
    let client = redis::Client::open("redis://127.0.0.1").unwrap();
    let manager = RedisConnectionManager::new_with_executor(client, executor.clone());
    let pool = Pool::builder().max_size(40).build(manager).await?;

    for _ in 0..MAX {
        let pool = pool.clone();
        let tx = sender.clone();
        let task = single_request(pool, tx).map(|_| ());
        executor.spawn(Box::pin(task));
    }
    Ok(())
}

async fn try_main(executor: TaskExecutor) -> Result<(), Error<RedisError>> {
    let mark = Instant::now();
    let (tx, mut rx) = mpsc::channel::<()>(MAX);
    do_redis(executor, tx).await?;

    let mut num: usize = 0;
    while let Some(_) = rx.next().await {
        num += 1;
        if num == MAX {
            break;
        }
    }

    println!("cost {:?}", mark.elapsed());
    Ok(())
}

fn main() {
    env_logger::init();
    let mut rt = Runtime::new().unwrap();
    rt.block_on(try_main(rt.handle().clone())).unwrap();
}
