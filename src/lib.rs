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
//! ```
//!
//! # Metrics
//!
//! Mobc uses the metrics crate to expose the following metrics
//!
//! 1. Active Connections - The number of connections in use.
//! 1. Idle Connections - The number of connections that are not being used
//! 1. Wait Count - the number of processes waiting for a connection
//! 1. Wait Duration - A cumulative histogram of the wait time for a connection
//!

#![cfg_attr(feature = "docs", feature(doc_cfg))]
#![warn(missing_docs)]
#![recursion_limit = "256"]
mod config;

mod conn;
mod error;
mod metrics_utils;
#[cfg(feature = "unstable")]
#[cfg_attr(feature = "docs", doc(cfg(unstable)))]
pub mod runtime;
mod spawn;
mod time;

pub use error::Error;

pub use async_trait::async_trait;
pub use config::Builder;
use config::{Config, InternalConfig, ShareConfig};
use conn::{ActiveConn, ConnState, IdleConn};
use futures_channel::mpsc::{self, Receiver, Sender};
use futures_util::lock::{Mutex, MutexGuard};
use futures_util::select;
use futures_util::FutureExt;
use futures_util::SinkExt;
use futures_util::StreamExt;
use metrics::gauge;
use metrics_utils::{get_manager_type, DurationHistogramGuard};
pub use spawn::spawn;
use std::fmt;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Weak,
};
use std::time::{Duration, Instant};
#[doc(hidden)]
pub use time::{delay_for, interval};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::metrics_utils::{GaugeGuard, IDLE_CONNECTIONS, WAIT_COUNT, WAIT_DURATION};

const CONNECTION_REQUEST_QUEUE_SIZE: usize = 10000;

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

    /// Determines if the connection is still connected to the database when check-out.
    ///
    /// A standard implementation would check if a simple query like `SELECT 1`
    /// succeeds.
    async fn check(&self, conn: Self::Connection) -> Result<Self::Connection, Self::Error>;

    /// *Quickly* determines a connection is still valid when check-in.
    #[inline]
    fn validate(&self, _conn: &mut Self::Connection) -> bool {
        true
    }
}

struct SharedPool<M: Manager> {
    config: ShareConfig,
    manager: M,
    internals: Mutex<PoolInternals<M>>,
    state: PoolState,
    semaphore: Arc<Semaphore>,
}

struct PoolInternals<M: Manager> {
    config: InternalConfig,
    free_conns: Vec<IdleConn<M>>,
    wait_duration: Duration,
    cleaner_ch: Option<Sender<()>>,
}

struct PoolState {
    num_open: Arc<AtomicU64>,
    max_lifetime_closed: AtomicU64,
    max_idle_closed: Arc<AtomicU64>,
    wait_count: AtomicU64,
}

impl<M: Manager> Drop for PoolInternals<M> {
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

impl<M: Manager> fmt::Debug for Pool<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pool")
    }
}

/// Information about the state of a `Pool`.
#[derive(Debug)]
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

impl<M: Manager> Drop for Pool<M> {
    fn drop(&mut self) {}
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
    /// 0 means unlimited (limited only by `max_open`), defaults to 2.
    pub async fn set_max_idle_conns(&self, n: u64) {
        let mut internals = self.0.internals.lock().await;
        internals.config.max_idle =
            if internals.config.max_open > 0 && n > internals.config.max_open {
                internals.config.max_open
            } else {
                n
            };

        let max_idle = internals.config.max_idle as usize;
        // Treat max_idle == 0 as unlimited
        if max_idle > 0 && internals.free_conns.len() > max_idle {
            internals.free_conns.truncate(max_idle);
        }
    }

    /// Sets the maximum lifetime of connections in the pool.
    ///
    /// Expired connections may be closed lazily before reuse.
    ///
    /// None means reuse forever.
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

        if max_lifetime.is_some()
            && self.0.state.num_open.load(Ordering::Relaxed) > 0
            && internals.cleaner_ch.is_none()
        {
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

        gauge!(IDLE_CONNECTIONS, "manager" => get_manager_type::<M>()).set(0.0);

        let (share_config, internal_config) = config.split();
        let internals = Mutex::new(PoolInternals {
            config: internal_config,
            free_conns: Vec::new(),
            wait_duration: Duration::from_secs(0),
            cleaner_ch: None,
        });

        let pool_state = PoolState {
            num_open: Arc::new(AtomicU64::new(0)),
            max_lifetime_closed: AtomicU64::new(0),
            wait_count: AtomicU64::new(0),
            max_idle_closed: Arc::new(AtomicU64::new(0)),
        };

        let shared = Arc::new(SharedPool {
            config: share_config,
            manager,
            internals,
            semaphore: Arc::new(Semaphore::new(max_open)),
            state: pool_state,
        });

        Pool(shared)
    }

    /// Returns a single connection by either opening a new connection
    /// or returning an existing connection from the connection pool. Conn will
    /// block until either a connection is returned or timeout.
    pub async fn get(&self) -> Result<Connection<M>, Error<M::Error>> {
        match self.0.config.get_timeout {
            Some(duration) => self.get_timeout(duration).await,
            None => self.inner_get_with_retries().await,
        }
    }

    /// Retrieves a connection from the pool, waiting for at most `timeout`
    ///
    /// The given timeout will be used instead of the configured connection
    /// timeout.
    pub async fn get_timeout(&self, duration: Duration) -> Result<Connection<M>, Error<M::Error>> {
        time::timeout(duration, self.inner_get_with_retries()).await
    }

    async fn inner_get_with_retries(&self) -> Result<Connection<M>, Error<M::Error>> {
        let mut try_times: u32 = 0;
        let config = &self.0.config;
        loop {
            try_times += 1;
            match self.get_connection().await {
                Ok(conn) => return Ok(conn),
                Err(Error::BadConn) => {
                    if try_times == config.max_bad_conn_retries {
                        return self.get_connection().await;
                    }
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn get_connection(&self) -> Result<Connection<M>, Error<M::Error>> {
        let _guard = GaugeGuard::<M>::increment(WAIT_COUNT);
        let c = self.get_or_create_conn().await?;

        let conn = Connection {
            pool: self.clone(),
            conn: Some(c),
        };

        Ok(conn)
    }

    async fn validate_conn(
        &self,
        internal_config: InternalConfig,
        conn: IdleConn<M>,
    ) -> Option<IdleConn<M>> {
        if conn.is_brand_new() {
            return Some(conn);
        }

        if conn.expired(internal_config.max_lifetime) {
            return None;
        }

        if conn.idle_expired(internal_config.max_idle_lifetime) {
            return None;
        }

        let needs_health_check = self.0.config.health_check
            && conn.needs_health_check(self.0.config.health_check_interval);

        if needs_health_check {
            let (raw, split) = conn.split_raw();
            let checked_raw = self.0.manager.check(raw).await.ok()?;
            let mut checked = split.restore(checked_raw);
            checked.mark_checked();
            return Some(checked);
        }
        Some(conn)
    }

    async fn get_or_create_conn(&self) -> Result<ActiveConn<M>, Error<M::Error>> {
        self.0.state.wait_count.fetch_add(1, Ordering::Relaxed);
        let wait_guard = DurationHistogramGuard::<M>::start(WAIT_DURATION);

        let semaphore = Arc::clone(&self.0.semaphore);
        let permit = semaphore
            .acquire_owned()
            .await
            .map_err(|_| Error::PoolClosed)?;

        self.0.state.wait_count.fetch_sub(1, Ordering::SeqCst);

        let mut internals = self.0.internals.lock().await;

        internals.wait_duration += wait_guard.into_elapsed();

        let conn = internals.free_conns.pop();
        let internal_config = internals.config.clone();
        drop(internals);

        if let Some(conn) = conn {
            if let Some(valid_conn) = self.validate_conn(internal_config, conn).await {
                return Ok(valid_conn.into_active(permit));
            }
        }

        self.open_new_connection(permit).await
    }

    async fn open_new_connection(
        &self,
        permit: OwnedSemaphorePermit,
    ) -> Result<ActiveConn<M>, Error<M::Error>> {
        log::debug!("creating new connection from manager");
        match self.0.manager.connect().await {
            Ok(c) => {
                self.0.state.num_open.fetch_add(1, Ordering::Relaxed);
                let state = ConnState::new(
                    Arc::clone(&self.0.state.num_open),
                    Arc::clone(&self.0.state.max_idle_closed),
                );
                Ok(ActiveConn::new(c, permit, state))
            }
            Err(e) => Err(Error::Inner(e)),
        }
    }

    /// Returns information about the current state of the pool.
    /// It is better to use the metrics than this method, this method
    /// requires a lock on the internals
    pub async fn state(&self) -> State {
        let internals = self.0.internals.lock().await;
        let num_free_conns = internals.free_conns.len() as u64;
        let wait_duration = internals.wait_duration;
        let max_open = internals.config.max_open;
        drop(internals);
        State {
            max_open,

            connections: self.0.state.num_open.load(Ordering::Relaxed),
            in_use: self.0.state.num_open.load(Ordering::Relaxed) - num_free_conns,
            idle: num_free_conns,

            wait_count: self.0.state.wait_count.load(Ordering::Relaxed),
            wait_duration,
            max_idle_closed: self.0.state.max_idle_closed.load(Ordering::Relaxed),
            max_lifetime_closed: self.0.state.max_lifetime_closed.load(Ordering::Relaxed),
        }
    }
}

async fn recycle_conn<M: Manager>(shared: &Arc<SharedPool<M>>, mut conn: ActiveConn<M>) {
    if conn_still_valid(shared, &mut conn) {
        conn.set_brand_new(false);
        let internals = shared.internals.lock().await;
        put_idle_conn::<M>(internals, conn);
    }
}

fn conn_still_valid<M: Manager>(shared: &Arc<SharedPool<M>>, conn: &mut ActiveConn<M>) -> bool {
    if !shared.manager.validate(conn.as_raw_mut()) {
        log::debug!("bad conn when check in");
        return false;
    }

    true
}

fn put_idle_conn<M: Manager>(mut internals: MutexGuard<'_, PoolInternals<M>>, conn: ActiveConn<M>) {
    let idle_conn = conn.into_idle();
    // Treat max_idle == 0 as unlimited idle connections.
    if internals.config.max_idle == 0
        || internals.config.max_idle > internals.free_conns.len() as u64
    {
        internals.free_conns.push(idle_conn);
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
    if shared.state.num_open.load(Ordering::Relaxed) == 0 || internals.config.max_lifetime.is_none()
    {
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

        if internals.free_conns[i].created_at() < expired {
            let c = internals.free_conns.swap_remove(i);
            closing.push(c);
            continue;
        }
        i += 1;
    }
    drop(internals);

    shared
        .state
        .max_lifetime_closed
        .fetch_add(closing.len() as u64, Ordering::Relaxed);
    true
}

/// A smart pointer wrapping a connection.
pub struct Connection<M: Manager> {
    pool: Pool<M>,
    conn: Option<ActiveConn<M>>,
}

impl<M: Manager> Connection<M> {
    /// Returns true is the connection is newly established.
    pub fn is_brand_new(&self) -> bool {
        self.conn.as_ref().unwrap().is_brand_new()
    }

    /// Unwraps the raw database connection.
    pub fn into_inner(mut self) -> M::Connection {
        self.conn.take().unwrap().into_raw()
    }
}

impl<M: Manager> Drop for Connection<M> {
    fn drop(&mut self) {
        let Some(conn) = self.conn.take() else {
            return;
        };

        let pool = Arc::clone(&self.pool.0);

        self.pool.0.manager.spawn_task(async move {
            recycle_conn(&pool, conn).await;
        });
    }
}

impl<M: Manager> Deref for Connection<M> {
    type Target = M::Connection;
    fn deref(&self) -> &Self::Target {
        self.conn.as_ref().unwrap().as_raw_ref()
    }
}

impl<M: Manager> DerefMut for Connection<M> {
    fn deref_mut(&mut self) -> &mut M::Connection {
        self.conn.as_mut().unwrap().as_raw_mut()
    }
}
