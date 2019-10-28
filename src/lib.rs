mod config;

use config::Builder;
use config::Config;
pub use futures;
use futures::channel::mpsc;
use futures::channel::oneshot::{self, Sender};
pub use futures::compat::Future01CompatExt;
pub use futures::compat::Stream01CompatExt;
use futures::lock::{Mutex, MutexGuard};
pub use futures::Future;
pub use futures::FutureExt;
use futures::StreamExt;
use log::debug;
use std::error;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio_executor::Executor as TkExecutor;
use tokio_timer::{delay, Interval};

static CONNECTION_ID: AtomicUsize = AtomicUsize::new(0);

pub enum Error<E> {
    Inner(E),
    Timeout,
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Error<E> {
        Error::Inner(e)
    }
}

// pub trait Executor: TkExecutor + Send + Sync + 'static + Clone {};

pub type AnyFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

pub trait ConnectionManager: Send + Sync + 'static {
    type Connection: Send + 'static;
    type Error: error::Error + Send + Sync + 'static;
    type Executor: TkExecutor + Send + Sync + 'static + Clone;

    fn get_executor(&self) -> Self::Executor;
    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error>;
    fn is_valid(&self, conn: Self::Connection) -> AnyFuture<Self::Connection, Self::Error>;
    fn has_broken(&self, conn: &mut Option<Self::Connection>) -> bool;
}

struct Conn<C> {
    raw: Option<C>,
    id: u64,
    birth: Instant,
}

struct IdleConn<C> {
    conn: Conn<C>,
    idle_start: Instant,
}

struct PoolInternals<C> {
    conns: mpsc::Sender<IdleConn<C>>,
    num_conns: u32,
    idle_conns: u32,
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
    conns: Mutex<mpsc::Receiver<IdleConn<M::Connection>>>,
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
    /// Creates a new connection pool with a default configuration.
    pub async fn new<E>(manager: M) -> Result<Pool<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        Pool::builder().build(manager).await
    }

    /// Returns a builder type to configure a new pool.
    pub fn builder() -> Builder<M> {
        Builder::new()
    }

    pub async fn new_inner(
        config: Config<M::Executor>,
        manager: M,
        reaper_rate: Duration,
    ) -> Pool<M> {
        let (recycle, conns) = mpsc::channel(config.max_size as usize);
        let internals = PoolInternals {
            conns: recycle,
            num_conns: 0,
            pending_conns: 0,
            idle_conns: 0,
            last_error: None,
        };

        let shared = Arc::new(SharedPool {
            config: config,
            manager: manager,
            internals: Mutex::new(internals),
            conns: Mutex::new(conns),
        });

        let mut internals = shared.internals.lock().await;
        establish_idle_connections(&shared, &mut internals);

        if shared.config.max_lifetime.is_some() || shared.config.idle_timeout.is_some() {
            // reap_connections(&shared, reaper_rate);
        }

        drop(internals);

        Pool(shared)
    }

    /// Retrieves a connection from the pool.
    ///
    /// Waits for at most the configured connection timeout before returning an
    /// error.
    pub async fn get<E>(&self) -> Result<PooledConnection<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        self.get_timeout(self.0.config.connection_timeout).await
    }

    /// Retrieves a connection from the pool, waiting for at most `timeout`
    ///
    /// The given timeout will be used instead of the configured connection
    /// timeout.
    pub async fn get_timeout<E>(&self, timeout: Duration) -> Result<PooledConnection<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        let start = Instant::now();
        let end = start + timeout;
        let timeout = delay(end);

        // println!("get timeout");
        let mut conns = self.0.conns.lock().await;

        futures::select! {
            () = timeout.fuse() => Err(Error::Timeout),
            r = conns.next() => match r {
                Some(conn) => {
                    let mut internals = self.0.internals.lock().await;
                    internals.idle_conns -= 1;
                    return Ok(PooledConnection {
                        pool: Some(self.clone()),
                        conn: Some(conn.conn),
                    })
                }
                None => Err(Error::Timeout),
            }
        }
    }

    /// Attempts to retrieve a connection from the pool if there is one
    /// available.
    ///
    /// Returns `None` if there are no idle connections available in the pool.
    /// This method will not block waiting to establish a new connection.
    pub async fn try_get(&self) -> Option<PooledConnection<M>> {
        let mut conns = self.0.conns.lock().await;
        match conns.try_next() {
            Ok(Some(conn)) => {
                let mut internals = self.0.internals.lock().await;
                internals.idle_conns -= 1;
                Some(PooledConnection {
                    pool: Some(self.clone()),
                    conn: Some(conn.conn),
                })
            }
            _ => None,
        }
    }

    async fn wait_for_initialization<E>(&self) -> Result<(), Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        debug!("waiting for initialization");
        let _end = Instant::now() + self.0.config.connection_timeout;
        let initial_size = self.0.config.min_idle.unwrap_or(self.0.config.max_size);

        loop {
            let internals = self.0.internals.lock().await;
            if internals.num_conns == initial_size {
                break;
            }
        }

        Ok(())
    }

    fn put_back(self, _checkout: Instant, mut conn: Conn<M::Connection>) {
        // let new_shared = Arc::downgrade(self);

        let _ = self.0.config.executor.clone().spawn(Box::pin(async move {
            // This is specified to be fast, but call it before locking anyways
            let broken = self.0.manager.has_broken(&mut conn.raw);

            let mut internals = self.0.internals.lock().await;
            if broken {
                drop_conns(&self.0, internals, vec![conn]);
                return;
            } else {
                let conn = IdleConn {
                    conn,
                    idle_start: Instant::now(),
                };
                internals.conns.try_send(conn).unwrap();
                internals.idle_conns += 1;
            }
        }));
    }

    /// Returns information about the current state of the pool.
    pub async fn state(&self) -> State {
        let internals = self.0.internals.lock().await;
        State {
            connections: internals.num_conns,
            idle_connections: internals.idle_conns,
            _p: (),
        }
    }
}

fn drop_conns<M>(
    shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<PoolInternals<M::Connection>>,
    conn: Vec<Conn<M::Connection>>,
) where
    M: ConnectionManager,
{
    internals.num_conns -= conn.len() as u32;
    establish_idle_connections(shared, &mut internals);
    drop(internals);
}

fn establish_idle_connections<M>(
    shared: &Arc<SharedPool<M>>,
    internals: &mut PoolInternals<M::Connection>,
) where
    M: ConnectionManager,
{
    let min = shared.config.min_idle.unwrap_or(shared.config.max_size);
    let idle = internals.idle_conns as u32;
    debug!(
        "idle {} min {}, {}, {}",
        idle, min, internals.num_conns, internals.pending_conns,
    );
    for _ in idle..min {
        add_connection(shared, internals);
    }
}

fn add_connection<M>(shared: &Arc<SharedPool<M>>, internals: &mut PoolInternals<M::Connection>)
where
    M: ConnectionManager,
{
    if internals.num_conns + internals.pending_conns >= shared.config.max_size {
        return;
    }

    internals.pending_conns += 1;
    inner(Duration::from_secs(0), shared);

    fn inner<M>(_delay: Duration, shared: &Arc<SharedPool<M>>)
    where
        M: ConnectionManager,
    {
        debug!("inner add connection");
        let new_shared = Arc::downgrade(shared);
        let _ = shared.config.executor.clone().spawn(Box::pin(async move {
            let shared = match new_shared.upgrade() {
                Some(shared) => shared,
                None => return,
            };

            let conn = shared.manager.connect().await;
            match conn {
                Ok(conn) => {
                    debug!("adding connection");
                    let id = CONNECTION_ID.fetch_add(1, Ordering::Relaxed) as u64;
                    let mut internals = shared.internals.lock().await;

                    internals.last_error = None;
                    let now = Instant::now();
                    let mut conn = IdleConn {
                        conn: Conn {
                            raw: Some(conn),
                            birth: now,
                            id,
                        },
                        idle_start: now,
                    };

                    loop {
                        match internals.conns.try_send(conn) {
                            Ok(()) => break,
                            Err(c) => conn = c.into_inner(),
                        }
                    }
                    internals.pending_conns -= 1;
                    internals.idle_conns += 1;
                    internals.num_conns += 1;
                    drop(internals);
                }
                Err(err) => {
                    shared.internals.lock().await.last_error = Some(err.to_string());
                    let delay = Duration::from_millis(200);
                    inner(delay, &shared);
                }
            }
        }));
    }
}

// fn reap_connections<M>(shared: &Arc<SharedPool<M>>, reaper_rate: Duration)
// where
//     M: ConnectionManager,
// {
//     let new_shared = Arc::downgrade(shared);
//     let _ = shared
//         .manager
//         .get_executor()
//         .clone()
//         .spawn(Box::pin(async move {
//             while let Some(_) = Interval::new_interval(reaper_rate).next().await {
//                 reap_conn(&new_shared).await;
//             }
//         }));

//     async fn reap_conn<M>(shared: &Weak<SharedPool<M>>)
//     where
//         M: ConnectionManager,
//     {
//         let shared = match shared.upgrade() {
//             Some(shared) => shared,
//             None => return,
//         };

//         let mut old = Vec::with_capacity(shared.config.max_size as usize);
//         let mut to_drop = vec![];

//         let mut internals = shared.internals.lock().await;
//         std::mem::swap(&mut old, &mut internals.conns);
//         let now = Instant::now();
//         for conn in old {
//             let mut reap = false;
//             if let Some(timeout) = shared.config.idle_timeout {
//                 reap |= now - conn.idle_start >= timeout;
//             }
//             if let Some(lifetime) = shared.config.max_lifetime {
//                 reap |= now - conn.conn.birth >= lifetime;
//             }
//             if reap {
//                 to_drop.push(conn.conn);
//             } else {
//                 internals.conns.push(conn);
//             }
//         }
//         drop_conns(&shared, internals, to_drop);
//     }
// }

/// Information about the state of a `Pool`.
pub struct State {
    /// The number of connections currently being managed by the pool.
    pub connections: u32,
    /// The number of idle connections.
    pub idle_connections: u32,
    _p: (),
}

impl fmt::Debug for State {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("State")
            .field("connections", &self.connections)
            .field("idle_connections", &self.idle_connections)
            .finish()
    }
}

pub struct PooledConnection<M>
where
    M: ConnectionManager,
{
    pool: Option<Pool<M>>,
    conn: Option<Conn<M::Connection>>,
}

impl<M> PooledConnection<M>
where
    M: ConnectionManager,
{
    pub fn take_raw_conn(&mut self) -> M::Connection {
        self.conn.as_mut().unwrap().raw.take().unwrap()
    }

    pub fn set_raw_conn(&mut self, raw: M::Connection) {
        self.conn.as_mut().unwrap().raw = Some(raw);
    }
}

impl<M> Drop for PooledConnection<M>
where
    M: ConnectionManager,
{
    fn drop(&mut self) {
        self.pool
            .take()
            .unwrap()
            .put_back(Instant::now(), self.conn.take().unwrap());
    }
}

impl<M> Deref for PooledConnection<M>
where
    M: ConnectionManager,
{
    type Target = M::Connection;
    fn deref(&self) -> &Self::Target {
        &self.conn.as_ref().unwrap().raw.as_ref().unwrap()
    }
}

impl<M> DerefMut for PooledConnection<M>
where
    M: ConnectionManager,
{
    fn deref_mut(&mut self) -> &mut M::Connection {
        self.conn.as_mut().unwrap().raw.as_mut().unwrap()
    }
}
