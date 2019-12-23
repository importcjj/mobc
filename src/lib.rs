//! A generic connection pool with async/await support.
//!
//! Opening a new database connection every time one is needed is both
//! inefficient and can lead to resource exhaustion under high traffic
//! conditions. A connection pool maintains a set of open connections to a
//! database, handing them out for repeated use.
//!
//! mobc is agnostic to the connection type it is managing. Implementors of the
//! `ConnectionManager` trait provide the database-specific logic to create and
//! check the health of connections.
//!
//! # Example
//!
//! Using an imaginary "foodb" database.
//!
//! ```rust
//!use mobc::{Manager, Pool, ResultFuture};
//!
//!#[derive(Debug)]
//!struct FooError;
//!
//!struct FooConnection;
//!
//!impl FooConnection {
//!    async fn query(&self) -> String {
//!        "nori".to_string()
//!    }
//!}
//!
//!struct FooManager;
//!
//!impl Manager for FooManager {
//!    type Connection = FooConnection;
//!    type Error = FooError;
//!
//!    fn connect(&self) -> ResultFuture<Self::Connection, Self::Error> {
//!        Box::pin(futures::future::ok(FooConnection))
//!    }
//!
//!    fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error> {
//!        Box::pin(futures::future::ok(conn))
//!    }
//!}
//!
//!#[tokio::main]
//!async fn main() {
//!    let pool = Pool::builder().max_open(15).build(FooManager);
//!    let num: usize = 10000;
//!    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(16);
//!
//!    for _ in 0..num {
//!        let pool = pool.clone();
//!        let mut tx = tx.clone();
//!        tokio::spawn(async move {
//!            let conn = pool.get().await.unwrap();
//!            let name = conn.query().await;
//!            assert_eq!(name, "nori".to_string());
//!            tx.send(()).await.unwrap();
//!        });
//!    }
//!
//!    for _ in 0..num {
//!        rx.recv().await.unwrap();
//!    }
//!}
//!
//! ```
#![warn(missing_docs)]
#![recursion_limit = "256"]
mod config;
#[cfg(feature = "unstable")]
#[cfg_attr(docsrs, doc(cfg(feature = "unstable")))]
pub mod runtime;
mod spawn;
mod time;

pub use config::Builder;
use config::Config;
use futures::channel::mpsc::{self, Sender};
use futures::channel::oneshot::{self, Sender as ReqSender};
use futures::lock::{Mutex, MutexGuard};
use futures::select;
use futures::FutureExt;
use futures::SinkExt;
use futures::StreamExt;
use spawn::spawn;
use std::collections::HashMap;
use std::error;
use std::fmt;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
#[doc(hidden)]
pub use time::{delay_for, interval};

const CONNECTION_REQUEST_QUEUE_SIZE: usize = 10000;

/// The error type returned by methods in this crate.
pub enum Error<E> {
    /// Manager Errors
    Inner(E),
    /// Timeout
    Timeout,
    /// BadConn
    BadConn,
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Error<E> {
        Error::Inner(e)
    }
}

impl<E> fmt::Display for Error<E>
where
    E: fmt::Display + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Inner(ref err) => write!(f, "{}", err),
            Error::Timeout => write!(f, "Timed out in mobc"),
            Error::BadConn => write!(f, "Bad connection in mobc"),
        }
    }
}

impl<E> fmt::Debug for Error<E>
where
    E: fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Inner(ref err) => write!(f, "{:?}", err),
            Error::Timeout => write!(f, "Timed out in mobc"),
            Error::BadConn => write!(f, "Bad connection in mobc"),
        }
    }
}

impl<E> error::Error for Error<E>
where
    E: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            Error::Inner(ref err) => Some(err),
            Error::Timeout => None,
            Error::BadConn => None,
        }
    }
}

/// Result Future `Pin<Box<dyn Future<Output = Result<T, E>> + Send>>`
pub type ResultFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

/// A trait which provides connection-specific functionality.
pub trait Manager: Send + Sync + 'static {
    /// The connection type this manager deals with.
    type Connection: Send + 'static;
    /// The error type returned by `Connection`s.
    type Error: Send + Sync + 'static;

    /// Spawns a new asynchronous task.
    fn spawn_task<T>(&self, task: T)
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        spawn(task);
    }

    /// Attempts to create a new connection.
    fn connect(&self) -> ResultFuture<Self::Connection, Self::Error>;
    /// Determines if the connection is still connected to the database.
    ///
    /// A standard implementation would check if a simple query like `SELECT 1`
    /// succeeds.
    fn check(&self, conn: Self::Connection) -> ResultFuture<Self::Connection, Self::Error>;
}

struct SharedPool<M: Manager> {
    config: Config,
    manager: M,
    internals: Mutex<PoolInternals<M::Connection, M::Error>>,
}

struct Conn<C, E> {
    raw: Option<C>,
    #[allow(dead_code)]
    last_err: Mutex<Option<E>>,
    created_at: Instant,
}

impl<C, E> Conn<C, E> {
    fn close(&self, internals: &mut MutexGuard<'_, PoolInternals<C, E>>) {
        internals.num_open -= 1;
    }

    fn expired(&self, timeout: Option<Duration>) -> bool {
        match timeout {
            Some(dur) => self.created_at < Instant::now() - dur,
            None => false,
        }
    }
}

struct PoolInternals<C, E> {
    opener_ch: Sender<()>,
    free_conns: Vec<Conn<C, E>>,
    conn_requests: HashMap<u64, ReqSender<Result<Conn<C, E>, E>>>,
    num_open: u64,
    max_lifetime_closed: u64,
    max_idle_closed: u64,
    next_request_id: u64,
    wait_count: u64,
    wait_duration: Duration,
}

impl<C, E> Drop for PoolInternals<C, E> {
    fn drop(&mut self) {
        log::debug!("Pool internal drop");
    }
}

/// A generic connection pool.
pub struct Pool<M: Manager>(Arc<SharedPool<M>>);

/// Returns a new `Pool` referencing the same state as `self`.
impl<M: Manager> Clone for Pool<M> {
    fn clone(&self) -> Self {
        Pool(self.0.clone())
    }
}

/// Information about the state of a `Pool`.
pub struct State {
    /// Maximum number of open connections to the database
    pub max_open: u64,

    // Pool Status
    /// The number of established connections both in use and idle.
    pub connections: u64,
    /// The number of connections currently in use.
    pub in_use: u64,
    /// The number of idle connections.
    pub idle: u64,

    // Counters
    /// The total number of connections waited for.
    pub wait_count: u64,
    /// The total time blocked waiting for a new connection.
    pub wait_duration: Duration,
    /// The total number of connections closed due to `max_idle`.
    pub max_idle_closed: u64,
    /// The total number of connections closed due to `max_lifetime`.
    pub max_lifetime_closed: u64,
}

#[derive(PartialEq)]
enum GetStrategy {
    CachedOrNewConn,
    AlwaysNewConn,
}

impl<M: Manager> Drop for Pool<M> {
    fn drop(&mut self) {
        // println!("Pool dropped");
    }
}

impl<M: Manager> Pool<M> {
    /// Creates a new connection pool with a default configuration.
    pub fn new(manager: M) -> Pool<M> {
        Pool::builder().build(manager)
    }

    /// Returns a builder type to configure a new pool.
    pub fn builder() -> Builder<M> {
        Builder::new()
    }

    pub(crate) fn new_inner(manager: M, config: Config) -> Self {
        let max_open = if config.max_open == 0 {
            CONNECTION_REQUEST_QUEUE_SIZE
        } else {
            config.max_open as usize
        };
        let (opener_ch_sender, mut opener_ch) = mpsc::channel(max_open);
        let internals = Mutex::new(PoolInternals {
            free_conns: vec![],
            conn_requests: HashMap::new(),
            num_open: 0,
            max_lifetime_closed: 0,
            next_request_id: 0,
            wait_count: 0,
            max_idle_closed: 0,
            opener_ch: opener_ch_sender,
            wait_duration: Duration::from_secs(0),
        });
        let shared = Arc::new(SharedPool {
            config,
            manager,
            internals,
        });

        let shared1 = Arc::downgrade(&shared);
        shared.manager.spawn_task(async move {
            while let Some(_) = opener_ch.next().await {
                open_new_connection(&shared1).await;
            }
        });

        if let Some(max_lifetime) = shared.config.max_lifetime {
            let clean_rate = shared.config.clean_rate;
            let shared1 = Arc::downgrade(&shared);
            shared.manager.spawn_task(async move {
                connection_cleaner(shared1, clean_rate, max_lifetime).await;
            })
        }

        Pool(shared)
    }

    /// Returns a single connection by either opening a new connection
    /// or returning an existing connection from the connection pool. Conn will
    /// block until either a connection is returned or timeout.
    pub async fn get(&self) -> Result<Connection<M>, Error<M::Error>> {
        self.get_timeout(self.0.config.get_timeout).await
    }

    /// Retrieves a connection from the pool, waiting for at most `timeout`
    ///
    /// The given timeout will be used instead of the configured connection
    /// timeout.
    pub async fn get_timeout(&self, timeout: Duration) -> Result<Connection<M>, Error<M::Error>> {
        let mut try_times: u32 = 0;
        let config = &self.0.config;
        loop {
            try_times += 1;
            match self
                .inner_get_timeout(GetStrategy::CachedOrNewConn, timeout)
                .await
            {
                Ok(conn) => return Ok(conn),
                Err(Error::BadConn) => {
                    if try_times == config.max_bad_conn_retries {
                        return self
                            .inner_get_timeout(GetStrategy::AlwaysNewConn, timeout)
                            .await;
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn inner_get_timeout(
        &self,
        strategy: GetStrategy,
        dur: Duration,
    ) -> Result<Connection<M>, Error<M::Error>> {
        let timeout = delay_for(dur);
        let config = &self.0.config;

        let mut internals = self.0.internals.lock().await;
        let num_free = internals.free_conns.len();
        if strategy == GetStrategy::CachedOrNewConn && num_free > 0 {
            let c = internals.free_conns.swap_remove(0);

            if c.expired(config.max_lifetime) {
                c.close(&mut internals);
                return Err(Error::BadConn);
            }

            drop(internals);
            let pooled = Connection {
                pool: Some(self.clone()),
                conn: Some(c),
            };
            return Ok(pooled);
        }

        if config.max_open > 0 {
            if internals.num_open >= config.max_open {
                let (req_sender, req_recv) = oneshot::channel();
                let req_key = internals.next_request_id;
                internals.next_request_id += 1;
                internals.wait_count += 1;
                internals.conn_requests.insert(req_key, req_sender);
                // release
                drop(internals);

                let wait_start = Instant::now();
                select! {
                    () = timeout.fuse() => {
                        let mut internals = self.0.internals.lock().await;
                        internals.conn_requests.remove(&req_key);
                        internals.wait_duration += wait_start.elapsed();

                        return Err(Error::Timeout)
                    }
                    conn = req_recv.fuse() => {
                        match conn.unwrap() {
                            Ok(c) => {
                                if c.expired(config.max_lifetime) {
                                    let mut internals = self.0.internals.lock().await;
                                    c.close(&mut internals);
                                    return Err(Error::BadConn);
                                }
                                let pooled = Connection {
                                    pool: Some(self.clone()),
                                    conn: Some(c),
                                };
                                return Ok(pooled)
                            }
                            err @ Err(_) => return Err(Error::BadConn)
                        }
                    }
                }
            }
        }

        internals.num_open += 1;
        drop(internals);

        log::debug!("get conn with manager create");
        match self.0.manager.connect().await {
            Ok(c) => {
                let conn = Conn {
                    raw: Some(c),
                    last_err: Mutex::new(None),
                    created_at: Instant::now(),
                };
                let pooled = Connection {
                    pool: Some(self.clone()),
                    conn: Some(conn),
                };

                return Ok(pooled);
            }
            Err(e) => {
                let internals = self.0.internals.lock().await;
                maybe_open_new_connection(&self.0, internals).await;
                return Err(Error::Inner(e));
            }
        }
    }

    /// Returns information about the current state of the pool.
    pub async fn state(&self) -> State {
        let internals = self.0.internals.lock().await;
        State {
            max_open: self.0.config.max_open,

            connections: internals.num_open,
            in_use: internals.num_open - internals.free_conns.len() as u64,
            idle: internals.free_conns.len() as u64,

            wait_count: internals.wait_count,
            wait_duration: internals.wait_duration,
            max_idle_closed: internals.max_idle_closed,
            max_lifetime_closed: internals.max_lifetime_closed,
        }
    }
}

async fn recycle_conn<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    mut conn: Conn<M::Connection, M::Error>,
) {
    let raw_conn = conn.raw.take().unwrap();
    let checked = shared.manager.check(raw_conn).await;
    let conn = match checked {
        Ok(c) => {
            conn.raw = Some(c);
            Ok(conn)
        }
        Err(e) => Err(e),
    };

    let internals = shared.internals.lock().await;
    put_conn(&shared, internals, conn).await;
}

async fn open_new_connection<M: Manager>(shared: &Weak<SharedPool<M>>) {
    let shared = match shared.upgrade() {
        Some(shared) => shared,
        None => return,
    };

    let create_r = shared.manager.connect().await;
    let mut internals = shared.internals.lock().await;

    let c = match create_r {
        Ok(c) => {
            let conn = Conn {
                raw: Some(c),
                last_err: Mutex::new(None),
                created_at: Instant::now(),
            };
            Ok(conn)
        }
        Err(e) => {
            internals.num_open -= 1;
            Err(e)
        }
    };
    put_conn(&shared, internals, c).await;
}

async fn put_conn<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
    conn: Result<Conn<M::Connection, M::Error>, M::Error>,
) {
    if conn.is_err() {
        return maybe_open_new_connection(shared, internals).await;
    }

    let config = &shared.config;
    if config.max_open > 0 && internals.num_open > config.max_open {
        return;
    }

    if internals.conn_requests.len() > 0 {
        let key = internals.conn_requests.keys().next().unwrap().clone();
        let req = internals.conn_requests.remove(&key).unwrap();

        // FIXME
        if let Err(Ok(conn)) = req.send(conn) {
            return put_idle_conn(shared, internals, conn);
        }
        return;
    }

    if let Ok(conn) = conn {
        return put_idle_conn(shared, internals, conn);
    }
}

fn put_idle_conn<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
    conn: Conn<M::Connection, M::Error>,
) {
    let config = &shared.config;
    if config.max_idle > internals.free_conns.len() as u64 {
        internals.free_conns.push(conn)
    } else {
        internals.max_idle_closed += 1;
        conn.close(&mut internals)
    }
}

async fn maybe_open_new_connection<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
) {
    let mut num_requests = internals.conn_requests.len() as u64;
    let config = &shared.config;
    if config.max_open > 0 {
        let num_can_open = config.max_open - internals.num_open;
        if num_requests > num_can_open {
            num_requests = num_can_open;
        }
    }

    while num_requests > 0 {
        internals.num_open += 1;
        num_requests -= 1;
        // FIXME
        let _ = internals.opener_ch.send(());
    }
}

async fn connection_cleaner<M: Manager>(
    shared: Weak<SharedPool<M>>,
    clean_rate: Duration,
    max_lifetime: Duration,
) {
    let mut interval = interval(clean_rate);
    interval.tick().await;

    while clean_connection(&shared, max_lifetime).await {
        interval.tick().await;
    }
}

async fn clean_connection<M: Manager>(
    shared: &Weak<SharedPool<M>>,
    max_lifetime: Duration,
) -> bool {
    let shared = match shared.upgrade() {
        Some(shared) => shared,
        None => {
            log::debug!("failed to start connection_cleaner");
            return false;
        }
    };

    let expired = Instant::now() - max_lifetime;
    let mut internals = shared.internals.lock().await;
    let mut closing = vec![];

    let mut i = 0;
    log::debug!(
        "clean connections, idle conns {}",
        internals.free_conns.len()
    );

    loop {
        if i >= internals.free_conns.len() {
            break;
        }

        if internals.free_conns[i].created_at < expired {
            let c = internals.free_conns.swap_remove(i);
            closing.push(c);
            continue;
        }
        i += 1;
    }

    internals.max_lifetime_closed += closing.len() as u64;
    for conn in closing {
        conn.close(&mut internals);
    }
    return true;
}

/// A smart pointer wrapping a connection.
pub struct Connection<M: Manager> {
    pool: Option<Pool<M>>,
    conn: Option<Conn<M::Connection, M::Error>>,
}

impl<M: Manager> Drop for Connection<M> {
    fn drop(&mut self) {
        let pool = self.pool.take().unwrap();
        let conn = self.conn.take().unwrap();
        // FIXME: No clone!
        pool.clone().0.manager.spawn_task(async move {
            recycle_conn(&pool.0, conn).await;
        });
    }
}

impl<M: Manager> Deref for Connection<M> {
    type Target = M::Connection;
    fn deref(&self) -> &Self::Target {
        &self.conn.as_ref().unwrap().raw.as_ref().unwrap()
    }
}

impl<M: Manager> DerefMut for Connection<M> {
    fn deref_mut(&mut self) -> &mut M::Connection {
        self.conn.as_mut().unwrap().raw.as_mut().unwrap()
    }
}
