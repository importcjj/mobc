mod config;

use config::Config;
pub use futures;
pub use futures::compat::Future01CompatExt;
pub use futures::compat::Stream01CompatExt;
pub use futures::Future;
pub use futures::FutureExt;
use std::error;
use std::fmt;
use std::marker::Unpin;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;
use tokio_executor::Executor as TkExecutor;

pub enum Error<E> {
    Inner(E),
    Timeout,
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Error<E> {
        Error::Inner(e)
    }
}

pub trait Executor: TkExecutor + Send + Sync + 'static + Clone {}

pub type AnyFuture<T, E> = Box<dyn Future<Output = Result<T, E>> + Unpin + Send>;

pub trait ConnectionManager {
    type Connection: Send + 'static;
    type Error: error::Error + Send + 'static;
    type Executor: Executor;

    fn get_executor(&self) -> Option<Self::Executor> {
        Some(tokio_executor::DefaultExecutor::current())
    }
    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error>;
    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error>;
    fn has_broken(&self, conn: &mut Self::Connection) -> bool;
}

struct Conn<C> {
    raw: C,
    id: u64,
    birth: Instant,
}

struct IdleConn<C> {
    conn: Conn<C>,
    idle_start: Instant,
}

struct PoolInternals<C> {
    conns: Vec<IdleConn<C>>,
    num_conns: u32,
    pending_conns: u32,
    last_error: Option<String>,
}

struct SharedPool<M>
where
    M: ConnectionManager,
{
    config: Config<M::Executor>,
    manager: M,
    internals: Mutex<PoolInternals<M::Connection>>,
}

/// A generic connection pool.
pub struct Pool<M>(Arc<SharedPool<M>>)
where
    M: ConnectionManager;

/// Returns a new `Pool` referencing the same state as `self`.
impl<M> Clone for Pool<M>
where
    M: ConnectionManager,
{
    fn clone(&self) -> Self {
        Pool(self.0.clone())
    }
}

impl<M> Pool<M>
where
    M: ConnectionManager,
{
    pub fn new_inner(config: Config<M::Executor>, manager: M) -> Pool<M> {
        let internals = PoolInternals {
            conns: Vec::with_capacity(config.max_size as usize),
            num_conns: 0,
            pending_conns: 0,
            last_error: None,
        };

        let shared = Arc::new(SharedPool {
            config: config,
            manager: manager,
            internals: Mutex::new(internals),
        });

        Pool(shared)
    }

    pub async fn get<E>(&self) -> Result<PooledConnection<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        Ok(PooledConnection {
            pool: self.clone(),
            conn: Some(self.0.manager.connect().await?),
        })
    }

    async fn wait_for_initialization<E>(&self) -> Result<(), Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        Ok(())
    }
}

async fn add_connection<M, E>(
    pool: &Arc<SharedPool<M>>,
    internals: &mut PoolInternals<M::Connection>,
) -> Result<(), Error<E>>
where
    M: ConnectionManager,
    Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
{
    Ok(())
}

pub struct PooledConnection<M>
where
    M: ConnectionManager,
{
    pool: Pool<M>,
    pub conn: Option<M::Connection>,
}

impl<M> Drop for PooledConnection<M>
where
    M: ConnectionManager,
{
    fn drop(&mut self) {
        println!("drop2");
    }
}

impl<M> Deref for PooledConnection<M>
where
    M: ConnectionManager,
{
    type Target = M::Connection;
    fn deref(&self) -> &Self::Target {
        &self.conn.as_ref().unwrap()
    }
}

impl<M> DerefMut for PooledConnection<M>
where
    M: ConnectionManager,
{
    fn deref_mut(&mut self) -> &mut M::Connection {
        self.conn.as_mut().unwrap()
    }
}
