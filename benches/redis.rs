#[macro_use]
extern crate criterion;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use mobc::Error;
use mobc::Future01CompatExt;
use mobc::Pool;
use mobc_redis::redis::{self, RedisError};
use mobc_redis::RedisConnectionManager;
use std::iter;
use tokio::prelude::*;
use tokio::runtime::Runtime;
use tokio::runtime::TaskExecutor;
use tokio::sync::mpsc;

async fn mobc_redis_ping(
    pool: Pool<RedisConnectionManager<TaskExecutor>>,
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

fn mobc_redis_bench(n: usize) {
    let rt = Runtime::new().unwrap();
    let client = redis::Client::open("redis://127.0.0.1").unwrap();
    let manager = RedisConnectionManager::new_with_executor(client, rt.executor());
    let (tx, mut rx) = mpsc::channel::<()>(n);

    rt.block_on(async {
        let pool = match Pool::builder()
            .max_size(40_u32)
            .build::<RedisError>(manager)
            .await
        {
            Ok(pool) => pool,
            Err(e) => {
                println!("error");
                return;
            }
        };

        for _ in 0..n {
            let pool = pool.clone();
            let tx = tx.clone();
            let _ = mobc_redis_ping(pool, tx).await;
        }

    });
}

fn redis_async_share_bench(n: usize) {
    let rt = Runtime::new().unwrap();
    let client = redis::Client::open("redis://127.0.0.1").unwrap();
    rt.block_on(async {
        let conn = client.get_shared_async_connection_with_executor(rt.executor()).compat().await?;
        for _ in 0..n {
            let conn = conn.clone();
            redis::cmd("PING")
                .query_async::<_, String>(conn)
                .compat()
                .await?;
        }
        Ok(())
    })
}

fn bench_redis(c: &mut Criterion) {
    let mut group = c.benchmark_group("Redis Ping");
    for i in [100].iter() {
        // group.bench_with_input(BenchmarkId::new("mobc_redis", i), i, |b, i| {
        //     b.iter(|| mobc_redis_bench(*i))
        // });
        group.bench_with_input(BenchmarkId::new("redis_async_share", i), i,
        |b, i| b.iter(|| redis_async_share_bench(*i)));
    }
    group.finish();
}

criterion_group!(benches, bench_redis);
criterion_main!(benches);
