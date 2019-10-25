use log::info;
use mobc::Error;
use mobc::Pool;
use mobc::{Future01CompatExt, FutureExt};
use mobc_redis::redis::{self, RedisError};
use mobc_redis::RedisConnectionManager;
use std::time::Duration;
use tokio::timer;

async fn do_redis() -> Result<(), Error<RedisError>> {
    let client = redis::Client::open("redis://127.0.0.1").unwrap();
    let manager = RedisConnectionManager::new(client);
    let pool = Pool::new(manager).await?;

    println!("pool was created");

    let max: usize = 10;

    async fn ping(pool: Pool<RedisConnectionManager>) -> Result<(), Error<RedisError>> {
        let mut conn = pool.get().await?;
        let raw_conn = conn.take_raw_conn();

        let (raw_conn, pong) = redis::cmd("PING")
            .query_async::<_, String>(raw_conn)
            .compat()
            .await?;
        println!("now has connection");
        conn.set_raw_conn(raw_conn);

        println!("{:?}", pong);
        assert_eq!("PONG", pong);
        Ok(())
    }

    for _ in 0..max {
        let pool = pool.clone();
        tokio::spawn(ping(pool).map(|_| ()));
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = do_redis().await {
        println!("some error");
    }
    loop {}
}
