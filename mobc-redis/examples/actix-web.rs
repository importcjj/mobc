use actix_web::{web, App, HttpServer, Responder};
use mobc_redis::RedisConnectionManager;
use redis::AsyncCommands;

type Pool = mobc::Pool<RedisConnectionManager>;

async fn query_name(pool: web::Data<Pool>) -> impl Responder {
    let mut conn = pool.get().await.unwrap();
    let () = conn.set("name", "mobc-redis").await.unwrap();
    let result = conn.get("name").await;

    match result {
        Ok(pong) => pong,
        Err(e) => format!("Server error: {:?}", e),
    }
}

#[actix_rt::main]
async fn main() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    let manager = RedisConnectionManager::new(client);
    let pool = Pool::builder().max_open(100).build(manager);

    HttpServer::new(move || {
        App::new()
            .data(pool.clone())
            .route("/", web::get().to(query_name))
    })
    .bind("127.0.0.1:7777")
    .unwrap()
    .run()
    .await
    .unwrap();
}
