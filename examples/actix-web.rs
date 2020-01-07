use actix_web::{web, App, HttpServer, Responder};
use mobc_redis::RedisConnectionManager;
use mobc_redis::{redis, Connection};

type Pool = mobc::Pool<RedisConnectionManager>;

async fn ping(pool: web::Data<Pool>) -> impl Responder {
    let mut conn = pool.get().await.unwrap();
    match redis::cmd("PING")
        .query_async::<_, String>(&mut conn as &mut Connection)
        .await
    {
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
            .route("/", web::get().to(ping))
    })
    .bind("127.0.0.1:7777")
    .unwrap()
    .run()
    .await
    .unwrap();
}
