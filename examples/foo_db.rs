use mobc::{Manager, Pool, ResultFuture};

#[derive(Debug)]
struct FooError;

struct FooConnection;

impl FooConnection {
    async fn query(&self) -> String {
        "nori".to_string()
    }
}

struct FooManager;

impl Manager for FooManager {
    type Connection = FooConnection;
    type Error = FooError;

    fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(FooConnection))
    }

    fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(conn))
    }
}

#[tokio::main]
async fn main() {
    let pool = Pool::builder().max_open(15).build(FooManager);
    let num: usize = 10000;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(16);

    for _ in 0..num {
        let pool = pool.clone();
        let mut tx = tx.clone();
        tokio::spawn(async move {
            let conn = pool.get().await.unwrap();
            let name = conn.query().await;
            assert_eq!(name, "nori".to_string());
            tx.send(()).await.unwrap();
        });
    }

    for _ in 0..num {
        rx.recv().await.unwrap();
    }
}
