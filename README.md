# mobc

r2d2 with async/await

[![Build Status](https://travis-ci.com/importcjj/mobc.svg?token=ZZrg3rRkUA8NUGrjEsU9&branch=master)](https://travis-ci.com/importcjj/mobc) [![crates.io](https://img.shields.io/badge/crates.io-0.1.0-%23dea584)](https://crates.io/crates/mobc)

[Documentation](https://docs.rs/mobc/0.1.0/mobc/)

## implementation

* mobc_redis  [![crates.io](https://img.shields.io/badge/crates.io-0.2.0-%23dea584)](https://crates.io/crates/mobc-redis)

* mobc_postgres [![crates.io](https://img.shields.io/badge/crates.io-0.2.1-%23dea584)](https://crates.io/crates/mobc-postgres)

**Note: mobc requires at least Rust 1.39.**

## Usage

```rust
use tokio;

extern crate mobc;
extern crate mobc_foodb;

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

## DIY

You can easily customize your own connection pool with the following trait `ConnectionManager`.

```rust
/// Future alias
pub type AnyFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

/// A trait which provides connection-specific functionality.
pub trait ConnectionManager: Send + Sync + 'static {
    /// The connection type this manager deals with.
    type Connection: Send + 'static;
    /// The error type returned by `Connection`s.
    type Error: error::Error + Send + Sync + 'static;
    /// The executor type this manager bases.
    type Executor: TkExecutor + Send + Sync + 'static + Clone;

    /// Get a future executor.
    fn get_executor(&self) -> Self::Executor;

    /// Attempts to create a new connection.
    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error>;

    /// Determines if the connection is still connected to the database.
    ///
    /// A standard implementation would check if a simple query like `SELECT 1`
    /// succeeds.
    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error>;
    /// *Quickly* determines if the connection is no longer usable.
    ///
    /// This will be called synchronously every time a connection is returned
    /// to the pool, so it should *not* block. If it returns `true`, the
    /// connection will be discarded.
    ///
    /// For example, an implementation might check if the underlying TCP socket
    /// has disconnected. Implementations that do not support this kind of
    /// fast health check may simply return `false`.
    fn has_broken(&self, conn: &mut Option<Self::Connection>) -> bool;
}
```


## benchmark

The benchmark is not ready