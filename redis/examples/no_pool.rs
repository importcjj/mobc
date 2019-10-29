use futures_01::future::Executor as Executor01;
use futures_01::future::{ExecuteError, ExecuteErrorKind};
use futures_01::Future as Future01;
use mobc::{Future01CompatExt, FutureExt};
use redis::Client;
use redis::{self, RedisError};
use std::time::Instant;
use tokio::prelude::*;

const MAX: usize = 5000;

async fn single_request(client: Client) -> Result<(), RedisError> {
    let conn = client.get_async_connection().compat().await?;
    let (_, pong) = redis::cmd("PING")
        .query_async::<_, String>(conn)
        .compat()
        .await?;
    assert_eq!("PONG", pong);
    Ok(())
}

#[tokio::main]
async fn main() {
    let mark = Instant::now();

    let client = redis::Client::open("redis://127.0.0.1").unwrap();

    for _ in 0..MAX {
        let client = client.clone();
        let _ = single_request(client).await;
    }

    println!("cost {:?}", mark.elapsed());
}
