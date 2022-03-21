# Prisma Mobc

This is a fork of the original Mobc with fixes for stablility. This changes the design of Mobc to use
a Semaphore to manage the pool rather than the original design that used channels.

A generic connection pool with async/await support.

Inspired by Deadpool, Sqlx, r2d2 and Golang SQL package.

[Changelog](https://github.com/prisma/mobc/blob/main/CHANGELOG.md)

**Note: mobc requires at least Rust 1.39.**

## Usage

```toml
[dependencies]
mobc = { git = "https://github.com/prisma/mobc",  branch = "main"}

# For async-std runtime
# mobc = { git = "https://github.com/prisma/mobc", features = ["async-std"] }

# For actix-rt 1.0
# mobc = { git = "https://github.com/prisma/mobc", features = ["actix-rt"] }
```

## Features

- Support async/.await syntax
- Support both `tokio` and `async-std`
- High performance
- Easy to customize
- Dynamic configuration

## Adaptors

| Backend                                                                                   | Adaptor Crate                                                           |
| ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| [tokio-postgres](https://github.com/sfackler/rust-postgres)                               | [mobc-postgres](https://github.com/importcjj/mobc-postgres)             |
| [redis](https://github.com/mitsuhiko/redis-rs)                                            | [mobc-redis](https://github.com/importcjj/mobc-redis)                   |
| [arangodb](https://github.com/fMeow/arangors)                                             | [mobc-arangors](https://github.com/inzanez/mobc-arangors)               |
| [lapin](https://github.com/CleverCloud/lapin)                                             | [mobc-lapin](https://github.com/zupzup/mobc-lapin)                      |
| [reql](https://github.com/rethinkdb/rethinkdb-rs)                                         | [mobc-reql](https://github.com/rethinkdb/rethinkdb-rs)                  |
| [redis-cluster](https://docs.rs/redis_cluster_async/0.6.0/redis_cluster_async/index.html) | [mobc-redis-cluster](https://github.com/rogeriob2br/mobc-redis-cluster) |

More DB adaptors are welcome.

## Examples

More [examples](https://github.com/importcjj/mobc/tree/master/mobc-foo/examples)

Using an imaginary "foodb" database.

```rust
use mobc::{async_trait, Manager};

#[derive(Debug)]
pub struct FooError;

pub struct FooConnection;

impl FooConnection {
    pub async fn query(&self) -> String {
        "PONG".to_string()
    }
}

pub struct FooManager;

#[async_trait]
impl Manager for FooManager {
    type Connection = FooConnection;
    type Error = FooError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        Ok(FooConnection)
    }

    async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
        Ok(conn)
    }
}
```

## Configures

#### max_open

Sets the maximum number of connections managed by the pool.

> 0 means unlimited, defaults to 10.

#### min_idle

Sets the maximum idle connection count maintained by the pool. The pool will maintain at most this many idle connections at all times, while respecting the value of max_open.

#### max_lifetime

Sets the maximum lifetime of connections in the pool. Expired connections may be closed lazily before reuse.

> None meas reuse forever, defaults to None.

#### get_timeout

Sets the get timeout used by the pool. Calls to Pool::get will wait this long for a connection to become available before returning an error.

> None meas never timeout, defaults to 30 seconds.

## Variable

Some of the connection pool configurations can be adjusted dynamically. Each connection pool instance has the following methods:

- set_max_open_conns
- set_max_idle_conns
- set_conn_max_lifetime

## Stats

- max_open - Maximum number of open connections to the database.
- connections - The number of established connections both in use and idle.
- in_use - The number of connections currently in use.
- idle - The number of idle connections.
- wait_count - The total number of connections waited for.
- wait_duration - The total time blocked waiting for a new connection.
- max_idle_closed - The total number of connections closed due to max_idle.
- max_lifetime_closed - The total number of connections closed due to max_lifetime.

## Compatibility

Because tokio is not compatible with other runtimes, such as async-std. So a database driver written with tokio cannot run in the async-std runtime. For example, you can't use redis-rs in tide because it uses tokio, so the connection pool which bases on redis-res can't be used in tide either.
