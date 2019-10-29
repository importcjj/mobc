use mobc::futures::FutureExt;
use std::str::FromStr;
use std::time::Instant;
use tokio_postgres::{tls::NoTls, Config, Error};

const MAX: usize = 5000;

async fn single_request(config: Config) -> Result<(), Error> {
    let (client, conn) = config.connect(NoTls).await?;
    tokio::spawn(conn.map(|_| ()));
    let r = client.execute("SELECT 1", &[]).await?;
    assert_eq!(r, 1);
    Ok(())
}

#[tokio::main]
async fn main() {
    let mark = Instant::now();

    let config = Config::from_str("postgres://jiaju:jiaju@localhost:5432").unwrap();
    for _ in 0..MAX {
        let config = config.clone();
        let _ = single_request(config).await;
    }

    println!("cost {:?}", mark.elapsed());
}
