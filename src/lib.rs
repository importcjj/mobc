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
use futures_channel::mpsc::{self, Receiver, Sender};
use futures_util::lock::{Mutex, MutexGuard};
use futures_util::select;
use futures_util::FutureExt;
use futures_util::SinkExt;
use futures_util::StreamExt;
use metrics::{decrement_gauge, gauge, histogram, increment_gauge};
pub use spawn::spawn;
use std::fmt;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
#[doc(hidden)]
pub use time::{delay_for, interval};
use tokio::sync::Semaphore;

use metrics_utils::{ACTIVE_CONNECTIONS, WAIT_COUNT, WAIT_DURATION};

use crate::metrics_utils::IDLE_CONNECTIONS;

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
    internals: Mutex<PoolInternals<M::Connection, M::Error>>,
    semaphore: Semaphore,
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
        timeout
            .and_then(|check_interval| {
                Instant::now()
                    .checked_duration_since(self.created_at)
                    .map(|dur_since| dur_since >= check_interval)
            })
            .unwrap_or(false)
    }

    fn idle_expired(&self, timeout: Option<Duration>) -> bool {
        timeout
            .and_then(|check_interval| {
                Instant::now()
                    .checked_duration_since(self.last_used_at)
                    .map(|dur_since| dur_since >= check_interval)
            })
            .unwrap_or(false)
    }

    fn needs_health_check(&self, timeout: Option<Duration>) -> bool {
        timeout
            .and_then(|check_interval| {
                Instant::now()
                    .checked_duration_since(self.last_checked_at)
                    .map(|dur_since| dur_since >= check_interval)
            })
            .unwrap_or(true)
    }
}

struct PoolInternals<C, E> {
    config: InternalConfig,
    free_conns: Vec<Conn<C, E>>,
    num_open: u64,
    max_lifetime_closed: u64,
    max_idle_closed: u64,
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

        gauge!(IDLE_CONNECTIONS, max_open as f64);

        let (share_config, internal_config) = config.split();
        let internals = Mutex::new(PoolInternals {
            config: internal_config,
            free_conns: vec![],
            num_open: 0,
            max_lifetime_closed: 0,
            wait_count: 0,
            max_idle_closed: 0,
            wait_duration: Duration::from_secs(0),
            cleaner_ch: None,
        });
        let shared = Arc::new(SharedPool {
            config: share_config,
            manager,
            internals,
            semaphore: Semaphore::new(max_open),
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
        let mut c = self.get_or_create_conn().await?;
        c.last_used_at = Instant::now();

        let conn = Connection {
            pool: Some(self.clone()),
            conn: Some(c),
        };

        increment_gauge!(ACTIVE_CONNECTIONS, 1.0);
        decrement_gauge!(WAIT_COUNT, 1.0);

        Ok(conn)
    }

    async fn validate_conn(
        &self,
        internals: &MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
        conn: &mut Conn<M::Connection, M::Error>,
    ) -> bool {
        if conn.brand_new {
            return true;
        }

        if conn.expired(internals.config.max_lifetime) {
            return false;
        }

        if conn.idle_expired(internals.config.max_idle_lifetime) {
            return false;
        }

        let needs_health_check = self.0.config.health_check
            && conn.needs_health_check(self.0.config.health_check_interval);

        if needs_health_check {
            let raw = conn.raw.take().unwrap();
            match self.0.manager.check(raw).await {
                Ok(raw) => {
                    conn.last_checked_at = Instant::now();
                    conn.raw = Some(raw)
                }
                Err(_e) => return false,
            }
        }
        true
    }

    async fn get_or_create_conn(&self) -> Result<Conn<M::Connection, M::Error>, Error<M::Error>> {
        let mut internals = self.0.internals.lock().await;
        internals.wait_count += 1;
        increment_gauge!(WAIT_COUNT, 1.0);
        let wait_start = Instant::now();

        drop(internals);
        let permit = self
            .0
            .semaphore
            .acquire()
            .await
            .map_err(|_| Error::PoolClosed)?;

        let mut internals = self.0.internals.lock().await;
        internals.wait_duration += wait_start.elapsed();
        histogram!(WAIT_DURATION, wait_start.elapsed());
        let conn = internals.free_conns.pop();

        if conn.is_some() {
            let mut conn = conn.unwrap();
            if self.validate_conn(&internals, &mut conn).await {
                decrement_gauge!(IDLE_CONNECTIONS, 1.0);
                permit.forget();
                return Ok(conn);
            } else {
                conn.close(&mut internals);
            }
        }

        let create_r = self.open_new_connection(internals).await;

        if create_r.is_ok() {
            decrement_gauge!(IDLE_CONNECTIONS, 1.0);
            permit.forget();
        }

        create_r
    }

    async fn open_new_connection(
        &self,
        mut internals: MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
    ) -> Result<Conn<M::Connection, M::Error>, Error<M::Error>> {
        log::debug!("creating new connection from manager");
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

                Ok(conn)
            }
            Err(e) => Err(Error::Inner(e)),
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
    decrement_gauge!(ACTIVE_CONNECTIONS, 1.0);
    let internals = shared.internals.lock().await;
    put_conn(shared, internals, conn).await;
}

async fn put_conn<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Connection, M::Error>>,
    mut conn: Conn<M::Connection, M::Error>,
) {
    if conn_still_valid(shared, &mut conn) {
        conn.brand_new = false;
        put_idle_conn(shared, internals, conn);
    } else {
        conn.close(&mut internals);
    }

    increment_gauge!(IDLE_CONNECTIONS, 1.0);
    shared.semaphore.add_permits(1);
}

fn conn_still_valid<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    conn: &mut Conn<M::Connection, M::Error>,
) -> bool {
    if conn.raw.is_none() {
        return false;
    }

    if !shared.manager.validate(conn.raw.as_mut().unwrap()) {
        log::debug!("bad conn when check in");
        return false;
    }

    true
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
    true
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

    /// Unwraps the raw database connection.
    pub fn into_inner(mut self) -> M::Connection {
        self.conn.as_mut().unwrap().raw.take().unwrap()
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
        self.conn.as_ref().unwrap().raw.as_ref().unwrap()
    }
}

impl<M: Manager> DerefMut for Connection<M> {
    fn deref_mut(&mut self) -> &mut M::Connection {
        self.conn.as_mut().unwrap().raw.as_mut().unwrap()
    }
}
