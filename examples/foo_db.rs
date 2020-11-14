mod foodb;

use foodb::FooManager;
use mobc::Pool;
use std::time::Instant;

#[tokio::main]
async fn main() {
    let pool = Pool::builder().max_open(15).build(FooManager);
    let num: usize = 10000;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(16);

    let now = Instant::now();
    for _ in 0..num {
        let pool = pool.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let conn = pool.get().await.unwrap();
            let name = conn.query().await;
            assert_eq!(name, "PONG".to_string());
            tx.send(()).await.unwrap();
        });
    }

    for _ in 0..num {
        rx.recv().await.unwrap();
    }

    println!("cost: {:?}", now.elapsed());
}
