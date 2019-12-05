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

If you are using tokio 0.2-alpha.6, use mobc 0.2.10.

```toml
[dependencies]
mobc = "0.3.0"
```

#### foo demo
```rust
use tokio;

#[tokio::main]
async fn main() {
    let manager = mobc_foodb::FooConnectionManager::new("localhost:1234");
    let pool = mobc::Pool::builder()
        .max_size(15)
        .build(manager)
        .await
        .unwrap();

    for _ in 0..20 {
        let pool = pool.clone();
        tokio::spawn(async {
            let conn = pool.get().await.unwrap();
            // use the connection
            // it will be returned to the pool when it falls out of scope.
        });
    }
}

```