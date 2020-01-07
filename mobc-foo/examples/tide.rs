use mobc_foo::FooManager;

use tide::Request;

type Pool = mobc::Pool<FooManager>;

async fn ping(req: Request<Pool>) -> String {
    let pool = req.state();
    let conn = pool.get().await.unwrap();
    conn.query().await
}
#[async_std::main]
async fn main() {
    let manager = FooManager;
    let pool = Pool::builder().max_open(100).build(manager);

    let mut app = tide::with_state(pool);
    app.at("/").get(ping);
    app.listen("127.0.0.1:7777").await.unwrap();
}