use mobc::{runtime::DefaultExecutor, AnyFuture, ConnectionManager, Pool};

struct FooManager;

impl ConnectionManager for FooManager {
    type Connection = FooConnection;
    type Error = std::io::Error;
    type Executor = DefaultExecutor;

    fn get_executor(&self) -> Self::Executor {
        DefaultExecutor::current()
    }

    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(FooConnection))
    }

    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(conn))
    }

    fn has_broken(&self, conn: &mut Option<Self::Connection>) -> bool {
        false
    }
}

struct FooConnection;

impl FooConnection {
    async fn query(&self) -> String {
        "nori".to_string()
    }
}

#[tokio::main]
async fn main() {
    let pool = mobc::Pool::builder()
        .max_size(15)
        .build(FooManager)
        .await
        .unwrap();

    let mut handles = vec![];

    for _ in 0..200 {
        let pool = pool.clone();
        let h = tokio::spawn(async move {
            let conn = pool.get().await.unwrap();
            let name = conn.query().await;
            assert_eq!(name, "nori".to_string());
        });

        handles.push(h)
    }

    for h in handles {
        h.await;
    }
}
