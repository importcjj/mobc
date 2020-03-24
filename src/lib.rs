//! A generic connection pool with async/await support.
//!
//! Opening a new database connection every time one is needed is both
//! inefficient and can lead to resource exhaustion under high traffic
//! conditions. A connection pool maintains a set of open connections to a
//! database, handing them out for repeated use.
//!
//! mobc is agnostic to the connection type it is managing. Implementors of the
//! `Manager` trait provide the database-specific logic to create and
//! check the health of connections.
//!
//! # Example
//!
//! Using an imaginary "foodb" database.
//!
//! ```rust
//!use mobc::{Manager, Pool, async_trait};
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
//!#[async_trait]
//!impl Manager for FooManager {
//!    type Connection = FooConnection;
//!    type Error = FooError;
//!
//!    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
//!        Ok(FooConnection)
//!    }
//!
//!    async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error> {
//!        Ok(conn)
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

#![cfg_attr(feature = "docs", feature(doc_cfg))]
#![warn(missing_docs)]
#![recursion_limit = "256"]
mod config;

#[cfg(feature = "unstable")]
#[cfg_attr(feature = "docs", doc(cfg(unstable)))]
pub mod runtime;
mod spawn;
mod time;

pub use async_trait::async_trait;
pub use config::Builder;
use config::{Config, InternalConfig, ShareConfig};
use futures::channel::mpsc::{self, Receiver, Sender};
use futures::channel::oneshot::{self, Sender as ReqSender};
use futures::lock::{Mutex, MutexGuard};
use futures::select;
use futures::FutureExt;
use futures::SinkExt;
use futures::StreamExt;
pub use spawn::spawn;
use std::collections::HashMap;
use std::error;
use std::fmt;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
#[doc(hidden)]
pub use time::{delay_for, delay_until, interval};

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

#[async_trait]
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
    async fn connect(&self) -> Result<Self::Connection, Self::Error>;
    /// Determines if the connection is still connected to the database.
    ///
    /// A standard implementation would check if a simple query like `SELECT 1`
    /// succeeds.
    async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error>;
}

struct SharedPool<M: Manager> {
    config: ShareConfig,
    manager: M,
    internals: Mutex<PoolInternals<M::Connection, M::Error>>,
}

struct Conn<C, E> {
    raw: Option<C>,
    #[allow(dead_code)]
    last_err: Mutex<Option<E>>,
    created_at: Instant,
    last_used_at: Instant,
    last_checked_at: Instant,
    brand_new: bool,
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

    fn idle_expired(&self, timeout: Option<Duration>) -> bool {
        timeout
            .map(|dur| self.last_used_at < Instant::now() - dur)
            .unwrap_or(false)
    }

    fn needs_health_check(&self, timeout: Option<Duration>) -> bool {
        timeout
            .map(|dur| self.last_checked_at < Instant::now() - dur)
            .unwrap_or(true)
    }
}

struct PoolInternals<C, E> {
    config: InternalConfig,
    opener_ch: Sender<()>,
    free_conns: Vec<Conn<C, E>>,
    conn_requests: HashMap<u64, ReqSender<Conn<C, E>>>,
    num_open: u64,
    max_lifetime_closed: u64,
    max_idle_closed: u64,
    next_request_id: u64,
    wait_count: u64,
    wait_duration: Duration,
    cleaner_ch: Option<Sender<()>>,
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

impl fmt::Debug for State {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Stats")
            .field("max_open", &self.max_open)
            .field("connections", &self.connections)
            .field("in_use", &self.in_use)
            .field("idle", &self.idle)
            .field("wait_count", &self.wait_count)
            .field("wait_duration", &self.wait_duration)
            .field("max_idle_closed", &self.max_idle_closed)
            .field("max_lifetime_closed", &self.max_lifetime_closed)
            .finish()
    }
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

    /// Sets the maximum number of connections managed by the pool.
    ///
    /// 0 means unlimited, defaults to 10.
    pub async fn set_max_open_conns(&self, n: u64) {
        let mut internals = self.0.internals.lock().await;
        internals.config.max_open = n;
        if n > 0 && internals.config.max_idle > n {
            drop(internals);
            self.set_max_idle_conns(n).await;
        }
    }

    /// Sets the maximum idle connection count maintained by the pool.
    ///
    /// The pool will maintain at most this many idle connections
    /// at all times, while respecting the value of `max_open`.
    ///
    /// Defaults to 2.
    pub async fn set_max_idle_conns(&self, n: u64) {
        let mut internals = self.0.internals.lock().await;
        internals.config.max_idle =
            if internals.config.max_open > 0 && n > internals.config.max_open {
                internals.config.max_open
            } else {
                n
            };

        let max_idle = internals.config.max_idle as usize;
        if internals.free_conns.len() > max_idle {
            let closing = internals.free_conns.split_off(max_idle);
            internals.max_idle_closed += closing.len() as u64;
            for conn in closing {
                conn.close(&mut internals);
            }
        }
    }

    /// Sets the maximum lifetime of connections in the pool.
    ///
    /// Expired connections may be closed lazily before reuse.
    ///
    /// None meas reuse forever.
    /// Defaults to None.
    ///
    /// # Panics
    ///
    /// Panics if `max_lifetime` is the zero `Duration`.
    pub async fn set_conn_max_lifetime(&self, max_lifetime: Option<Duration>) {
        assert_ne!(
            max_lifetime,
            Some(Duration::from_secs(0)),
            "max_lifetime must be positive"
        );
        let mut internals = self.0.internals.lock().await;
        internals.config.max_lifetime = max_lifetime;
        if let Some(lifetime) = max_lifetime {
            match internals.config.max_lifetime {
                Some(prev) if lifetime < prev && internals.cleaner_ch.is_some() => {
                    // FIXME
                    let _ = internals.cleaner_ch.as_mut().unwrap().send(()).await;
                }
                _ => (),
            }
        }

        if max_lifetime.is_some() && internals.num_open > 0 && internals.cleaner_ch.is_none() {
            log::debug!("run connection cleaner");
            let shared1 = Arc::downgrade(&self.0);
            let clean_rate = self.0.config.clean_rate;
            let (cleaner_ch_sender, cleaner_ch) = mpsc::channel(1);
            internals.cleaner_ch = Some(cleaner_ch_sender);
            self.0.manager.spawn_task(async move {
                connection_cleaner(shared1, cleaner_ch, clean_rate).await;
            });
        }
    }

    pub(crate) fn new_inner(manager: M, config: Config) -> Self {
        let max_open = if config.max_open == 0 {
            CONNECTION_REQUEST_QUEUE_SIZE
        } else {
            config.max_open as usize
        };
        let (opener_ch_sender, mut opener_ch) = mpsc::channel(max_open);
        let (share_config, internal_config) = config.split();
        let internals = Mutex::new(PoolInternals {
            config: internal_config,
            free_conns: vec![],
            conn_requests: HashMap::new(),
            num_open: 0,
            max_lifetime_closed: 0,
            next_request_id: 0,
            wait_count: 0,
            max_idle_closed: 0,
            opener_ch: opener_ch_sender,
            wait_duration: Duration::from_secs(0),
            cleaner_ch: None,
        });
        let shared = Arc::new(SharedPool {
            config: share_config,
            manager,
            internals,
        });

        let shared1 = Arc::downgrade(&shared);
        shared.manager.spawn_task(async move {
            while let Some(_) = opener_ch.next().await {
                open_new_connection(&shared1).await;
            }
        });

        Pool(shared)
    }

    /// Returns a single connection by either opening a new connection
    /// or returning an existing connection from the connection pool. Conn will
    /// block until either a connection is returned or timeout.
    pub async fn get(&self) -> Result<Connection<M>, Error<M::Error>> {
        match self.0.config.get_timeout {
            Some(duration) => self.get_timeout(duration).await,
            None => self.inner_get_never_timeout().await,
        }
    }

    /// Retrieves a connection from the pool, waiting for at most `timeout`
    ///
    /// The given timeout will be used instead of the configured connection
    /// timeout.
    pub async fn get_timeout(&self, duration: Duration) -> Result<Connection<M>, Error<M::Error>> {
        let mut try_times: u32 = 0;
        let deadline = Instant::now() + duration;

        let config = &self.0.config;
        loop {
            let timeout = delay_until(deadline);
            try_times += 1;
            match self.get_ctx(GetStrategy::CachedOrNewConn, timeout).await {
                Ok(conn) => return Ok(conn),
                Err(Error::BadConn) => {
                    if try_times == config.max_bad_conn_retries {
                        let timeout = delay_until(deadline);
                        return self.get_ctx(GetStrategy::AlwaysNewConn, timeout).await;
                    }
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn inner_get_never_timeout(&self) -> Result<Connection<M>, Error<M::Error>> {
        let mut try_times: u32 = 0;
        let config = &self.0.config;
        loop {
            let never_ot = futures::future::pending();
            try_times += 1;
            match self.get_ctx(GetStrategy::CachedOrNewConn, never_ot).await {
                Ok(conn) => return Ok(conn),
                Err(Error::BadConn) => {
                    if try_times == config.max_bad_conn_retries {
                        let never_ot = futures::future::pending();
                        return self.get_ctx(GetStrategy::AlwaysNewConn, never_ot).await;
                    }
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn get_ctx(
        &self,
        strategy: GetStrategy,
        ctx: impl Future<Output = ()> + Unpin,
    ) -> Result<Connection<M>, Error<M::Error>> {
        let mut c = self.inner_get_ctx(strategy, ctx).await?;

        if !c.brand_new {
            let mut internals = self.0.internals.lock().await;
            if c.expired(internals.config.max_lifetime) {
                c.close(&mut internals);
                return Err(Error::BadConn);
            }

            if c.idle_expired(internals.config.max_idle_lifetime) {
                c.close(&mut internals);
                return Err(Error::BadConn);
            }

            let needs_health_check = self.0.config.health_check
                && c.needs_health_check(self.0.config.health_check_interval);

            if needs_health_check {
                let raw = c.raw.take().unwrap();
                match self.0.manager.check(raw).await {
                    Ok(raw) => {
                        c.last_checked_at = Instant::now();
                        c.raw = Some(raw)
                    }
                    Err(e) => {
                        internals.num_open -= 1;
                        return Err(Error::Inner(e));
                    }
                }
            }
        }

        c.last_used_at = Instant::now();

        let conn = Connection {
            pool: Some(self.clone()),
            conn: Some(c),
        };

        Ok(conn)
    }

    async fn inner_get_ctx(
        &self,
        strategy: GetStrategy,
        ctx: impl Future<Output = ()> + Unpin,
    ) -> Result<Conn<M::Connection, M::Error>, Error<M::Error>> {
        let mut ctx = ctx.fuse();

        select! {
            () = ctx => {
                return Err(Error::Timeout)
            }
            default => ()
        }
        let mut internals = self.0.internals.lock().await;
        let num_free = internals.free_conns.len();
        if strategy == GetStrategy::CachedOrNewConn && num_free > 0 {
            let c = internals.free_conns.swap_remove(0);
            return Ok(c);
        }

        if internals.config.max_open > 0 {
            if internals.num_open >= internals.config.max_open {
                let (req_sender, req_recv) = oneshot::channel();
                let req_key = internals.next_request_id;
                internals.next_request_id += 1;
                internals.wait_count += 1;
                internals.conn_requests.insert(req_key, req_sender);
                // release
                drop(internals);

                let wait_start = Instant::now();
                select! {
                    () = ctx.fuse() => {
                        let mut internals = self.0.internals.lock().await;
                        internals.conn_requests.remove(&req_key);
                        internals.wait_duration += wait_start.elapsed();

                        return Err(Error::Timeout)
                    }
                    conn = req_recv.fuse() => {
                        let c = conn.unwrap();
                        return Ok(c)
                    }
                }
            }
        }

        log::debug!("get conn with manager create");
        match self.0.manager.connect().await {
            Ok(c) => {
                internals.num_open += 1;
                drop(internals);

                let conn = Conn {
                    raw: Some(c),
                    last_err: Mutex::new(None),
                    created_at: Instant::now(),
                    last_used_at: Instant::now(),
                    last_checked_at: Instant::now(),
                    brand_new: true,
                };

                return Ok(conn);
            }
            Err(e) => {
                maybe_open_new_connection(&self.0, internals).await;
                return Err(Error::Inner(e));
            }
        }
    }

    /// Returns information about the current state of the pool.
    pub async fn state(&self) -> State {
        let internals = self.0.internals.lock().await;
        State {
            max_open: internals.config.max_open,

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
    conn: Conn<M::Connection, M::Error>,
) {
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

    match create_r {
        Ok(c) => {
            let conn = Conn {
                raw: Some(c),
                last_err: Mutex::new(None),
                created_at: Instant::now(),
                last_used_at: Instant::now(),
                last_checked_at: Instant::now(),
                brand_new: true,
            };
            return put_conn(&shared, internals, conn).await;
        }
        Err(_) => {
            internals.num_open -= 1;
            return maybe_open_new_connection(&shared, internals).await;
        }
    };
}

async fn put_conn<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
    mut conn: Conn<M::Connection, M::Error>,
) {
    if conn.raw.is_none() {
        conn.close(&mut internals);
        return maybe_open_new_connection(shared, internals).await;
    }

    if internals.config.max_open > 0 && internals.num_open > internals.config.max_open {
        conn.close(&mut internals);
        return;
    }

    conn.brand_new = false;

    if internals.conn_requests.len() > 0 {
        let key = internals.conn_requests.keys().next().unwrap().clone();
        let req = internals.conn_requests.remove(&key).unwrap();

        // FIXME
        if let Err(conn) = req.send(conn) {
            return put_idle_conn(shared, internals, conn);
        }
        return;
    }

    return put_idle_conn(shared, internals, conn);
}

fn put_idle_conn<M: Manager>(
    _shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
    conn: Conn<M::Connection, M::Error>,
) {
    if internals.config.max_idle > internals.free_conns.len() as u64 {
        internals.free_conns.push(conn)
    } else {
        internals.max_idle_closed += 1;
        conn.close(&mut internals)
    }
}

async fn maybe_open_new_connection<M: Manager>(
    _shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
) {
    let mut num_requests = internals.conn_requests.len() as u64;
    if internals.config.max_open > 0 {
        let num_can_open = internals.config.max_open - internals.num_open;
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
    mut cleaner_ch: Receiver<()>,
    clean_rate: Duration,
) {
    let mut interval = interval(clean_rate);
    interval.tick().await;

    loop {
        select! {
            _ = interval.tick().fuse() => (),
            r = cleaner_ch.next().fuse() => match r{
                Some(()) => (),
                None=> return
            },
        }

        if !clean_connection(&shared).await {
            return;
        }
    }
}

async fn clean_connection<M: Manager>(shared: &Weak<SharedPool<M>>) -> bool {
    let shared = match shared.upgrade() {
        Some(shared) => shared,
        None => {
            log::debug!("Failed to clean connections");
            return false;
        }
    };

    log::debug!("Clean connections");

    let mut internals = shared.internals.lock().await;
    if internals.num_open == 0 || internals.config.max_lifetime.is_none() {
        internals.cleaner_ch.take();
        return false;
    }

    let expired = Instant::now() - internals.config.max_lifetime.unwrap();
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

impl<M: Manager> Connection<M> {
    /// Returns true is the connection is newly established.
    pub fn is_brand_new(&self) -> bool {
        self.conn.as_ref().unwrap().brand_new
    }
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
