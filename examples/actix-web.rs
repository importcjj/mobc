mod foodb;

use actix_web::{web, App, HttpServer, Responder};
use foodb::FooManager;

type Pool = mobc::Pool<FooManager>;

async fn ping(pool: web::Data<Pool>) -> impl Responder {
    let conn = pool.get().await.unwrap();
    conn.query().await
}

#[actix_web::main]
async fn main() {
    let pool = Pool::builder().max_open(100).build(FooManager);

    HttpServer::new(move || {
        App::new()
            .data(pool.clone())
            .route("/", web::get().to(ping))
    })
    .bind("127.0.0.1:7777")
    .unwrap()
    .run()
    .await
    .unwrap();
}
