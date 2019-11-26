use mobc::futures::channel::mpsc;
use mobc::futures::prelude::*;
use mobc::runtime::DefaultExecutor;
use mobc::runtime::Runtime;
use mobc::runtime::TaskExecutor;
use mobc::Error;
use mobc::Executor;
use mobc::Pool;
use mobc_postgres::tokio_postgres;
use mobc_postgres::PostgresConnectionManager;
use std::str::FromStr;
use std::time::Instant;
use tokio_postgres::Config;
use tokio_postgres::Error as PostgresError;
use tokio_postgres::NoTls;

const MAX: usize = 5000;

async fn simple_query(
    pool: Pool<PostgresConnectionManager<NoTls, TaskExecutor>>,
    mut sender: mpsc::Sender<()>,
) -> Result<(), Error<PostgresError>> {
    let conn = pool.get().await?;
    let r = conn.execute("SELECT 1", &[]).await?;
    assert_eq!(r, 1);
    sender.send(()).await.unwrap();
    Ok(())
}

async fn do_postgres(
    mut executor: TaskExecutor,
    sender: mpsc::Sender<()>,
) -> Result<(), Error<PostgresError>> {
    let config = Config::from_str("postgres://jiaju:jiaju@localhost:5432")?;
    let manager = PostgresConnectionManager::new_with_executor(config, NoTls, executor.clone());
    let pool = Pool::new(manager).await?;

    for _ in 0..MAX {
        let pool = pool.clone();
        let tx = sender.clone();
        let task = simple_query(pool, tx).map(|_| ());
        executor.spawn(Box::pin(task));
    }

    Ok(())
}

async fn try_main(executor: TaskExecutor) -> Result<(), Error<PostgresError>> {
    let mark = Instant::now();
    let (tx, mut rx) = mpsc::channel::<()>(MAX);

    do_postgres(executor, tx).await.unwrap();

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
    let rt = Runtime::new().unwrap();
    rt.block_on(try_main(rt.executor())).unwrap();
}
