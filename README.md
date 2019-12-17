# mobc

A generic connection pool, but async/.await

[![Build Status](https://travis-ci.com/importcjj/mobc.svg?token=ZZrg3rRkUA8NUGrjEsU9&branch=master)](https://travis-ci.com/importcjj/mobc) [![crates.io](https://img.shields.io/badge/crates.io-latest-%23dea584)](https://crates.io/crates/mobc)

[Documentation](https://docs.rs/mobc/latest/mobc/)

**Note: mobc requires at least Rust 1.39.**

## Features

* Support async/.await syntax.
* Support tokio 0.2 and async-std 1.0 runtimes.
* Simple and fast customization

## Adapter

* [mobc-redis = "0.3.1"](https://crates.io/crates/mobc-redis)

* [mobc-postgres = "0.3.1"](https://crates.io/crates/mobc-postgres)

## Usage

*If you are using tokio 0.2-alpha.6, use mobc 0.2*

```toml
[dependencies]
mobc = "0.3"
```

## Example

```rust
use mobc::{ConnectionManager, runtime::DefaultExecutor, Pool, AnyFuture};

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
```