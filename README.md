# mobc

A generic connection pool, but async/.await

[![Build Status](https://travis-ci.com/importcjj/mobc.svg?token=ZZrg3rRkUA8NUGrjEsU9&branch=0.4.x)](https://travis-ci.com/importcjj/mobc) [![crates.io](https://img.shields.io/badge/crates.io-latest-%23dea584)](https://crates.io/crates/mobc)

[Documentation](https://docs.rs/mobc/latest/mobc/)

**Note: mobc requires at least Rust 1.39.**

## Features

* Support async/.await syntax.
* Support tokio 0.2 and async-std 1.0 runtimes.
* Simple and fast customization


## Usage

*If you are using tokio 0.2-alpha.6, use mobc 0.2*

```toml
[dependencies]
mobc = "=0.4.0-alpha.0"
```

## Example

```rust
use mobc::{Manager, Config, Pool, ResultFuture};

struct FooManager;

impl Manager for FooManager {
    type Resource = FooConnection;
    type Error = std::io::Error;


    fn create(&self) -> ResultFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(FooConnection))
    }

    fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
        Box::pin(futures::future::ok(conn))
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
    let pool = Pool::new(FooManager, Config::default());

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