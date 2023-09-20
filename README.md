# Mobc

A generic connection pool with async/await support.

Inspired by Deadpool, Sqlx, r2d2 and Golang SQL package.

[Changelog](https://github.com/importcjj/mobc/blob/main/CHANGELOG.md)

**Note: mobc requires at least Rust 1.60.**

## Usage

```toml
[dependencies]
mobc = "0.8"

# For async-std runtime
# mobc = { version = "0.8", features = ["async-std"] }

# For actix-rt 1.0
# mobc = { version = "0.8", features = ["actix-rt"] }
```

## Features

- Support async/.await syntax
- Support both `tokio` and `async-std`
- Tokio metric support
- Production battle tested
- High performance
- Easy to customize
- Dynamic configuration

## Adaptors

| Backend                                                                                   | Adaptor Crate                                                           |
| ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| [bolt-client](https://crates.io/crates/bolt-client)                                       | [mobc-bolt](https://crates.io/crates/mobc-bolt)                         |
| [tokio-postgres](https://github.com/sfackler/rust-postgres)                               | [mobc-postgres](https://github.com/importcjj/mobc-postgres)             |
| [redis](https://github.com/mitsuhiko/redis-rs)                                            | [mobc-redis](https://github.com/importcjj/mobc-redis)                   |
| [arangodb](https://github.com/fMeow/arangors)                                             | [mobc-arangors](https://github.com/inzanez/mobc-arangors)               |
| [lapin](https://github.com/CleverCloud/lapin)                                             | [mobc-lapin](https://github.com/zupzup/mobc-lapin)                      |
| [reql](https://github.com/rethinkdb/rethinkdb-rs)                                         | [mobc-reql](https://github.com/rethinkdb/rethinkdb-rs)                  |
| [redis-cluster](https://docs.rs/redis_cluster_async/0.6.0/redis_cluster_async/index.html) | [mobc-redis-cluster](https://github.com/rogeriob2br/mobc-redis-cluster) |

More DB adaptors are welcome.

## Examples

More [examples](https://github.com/importcjj/mobc/tree/main/examples)

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

## Metrics

- Counters
    - `mobc_pool_connections_opened_total` - Total number of Pool Connections opened
    - `mobc_pool_connections_closed_total` - Total number of Pool Connections closed
- Gauges
    - `mobc_pool_connections_open` - Number of currently open Pool Connections
    - `mobc_pool_connections_busy` - Number of currently busy Pool Connections (executing a database query)"
    - `mobc_pool_connections_idle` - Number of currently unused Pool Connections (waiting for the next pool query to run)
    - `mobc_client_queries_wait` - Number of queries currently waiting for a connection
- Histograms
    - `mobc_client_queries_wait_histogram_ms` - Histogram of the wait time of all queries in ms
  
## Compatibility

Because tokio is not compatible with other runtimes, such as async-std. So a database driver written with tokio cannot run in the async-std runtime. For example, you can't use redis-rs in tide because it uses tokio, so the connection pool which bases on redis-res can't be used in tide either.
