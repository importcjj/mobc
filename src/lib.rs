mod spawn;
mod time;

use futures::channel::mpsc::{self, Sender};
use futures::channel::oneshot::{self, Sender as ReqSender};
use futures::lock::{Mutex, MutexGuard};
use futures::select;
use futures::FutureExt;
use futures::SinkExt;
use futures::StreamExt;
pub use spawn::spawn;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use time::{interval, timeout};

/// This is the size of the connectionOpener request chan (DB.openerCh).
/// This value should be larger than the maximum typical value
/// used for db.maxOpen. If maxOpen is significantly larger than
/// connectionRequestQueueSize then it is possible for ALL calls into the *DB
/// to block until the connectionOpener can satisfy the backlog of requests.
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

pub type ResultFuture<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

pub trait Manager: Send + Sync + 'static {
    type Resource: Send + 'static;
    type Error: Send + Sync + 'static;

    fn create(&self) -> ResultFuture<Self::Resource, Self::Error>;
    fn check(&self, conn: Self::Resource) -> ResultFuture<Self::Resource, Self::Error>;
}

pub struct Config {
    max_open: Option<u64>,
    max_idle: Option<u64>,
    max_lifetime: Option<Duration>,
    clean_rate: Duration,
    max_bad_conn_retries: u32,
    get_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_open: Some(16),
            max_idle: Some(16),
            max_lifetime: Some(Duration::from_secs(30 * 60)),
            clean_rate: Duration::from_secs(30),
            max_bad_conn_retries: 2,
            get_timeout: Duration::from_secs(30),
        }
    }
}

struct SharedPool<M: Manager> {
    config: Config,
    manager: M,
    internals: Mutex<PoolInternals<M::Resource, M::Error>>,
}

struct Conn<C, E> {
    raw: Option<C>,
    last_err: Mutex<Option<E>>,
    created_at: Instant,
}

impl<C, E> Conn<C, E> {
    fn expired(&self, timeout: Duration) -> bool {
        self.created_at < Instant::now() - timeout
    }
}

struct PoolInternals<C, E> {
    opener_ch: Sender<()>,
    free_conns: Vec<Conn<C, E>>,
    conn_requests: HashMap<u64, ReqSender<Result<Conn<C, E>, E>>>,
    num_open: u64,
    max_lifetime_closed: u64,
    next_request_id: u64,
    wait_count: u64,
    wait_duration: Duration,
}

pub struct Pool<M: Manager>(Arc<SharedPool<M>>);

impl<M: Manager> Clone for Pool<M> {
    fn clone(&self) -> Self {
        Pool(self.0.clone())
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
    pub fn new(manager: M, config: Config) -> Self {
        let max_open = config
            .max_open
            .unwrap_or(CONNECTION_REQUEST_QUEUE_SIZE as u64) as usize;
        let (opener_ch_sender, mut opener_ch) = mpsc::channel(max_open);
        let internals = Mutex::new(PoolInternals {
            free_conns: vec![],
            conn_requests: HashMap::new(),
            num_open: 0,
            max_lifetime_closed: 0,
            next_request_id: 0,
            wait_count: 0,
            opener_ch: opener_ch_sender,
            wait_duration: Duration::from_secs(0),
        });
        let shared = Arc::new(SharedPool {
            config,
            manager,
            internals,
        });

        let shared1 = Arc::downgrade(&shared);
        spawn(async move {
            while let Some(_) = opener_ch.next().await {
                open_new_connection(&shared1).await;
            }
        });


        Pool(shared)
    }

    async fn put_back_conn(mut self, conn: Conn<M::Resource, M::Error>) {
        spawn(async move {
            recycle_conn(&self.0, conn).await;
        });
    }

    pub async fn get(&self) -> Result<PooledObject<M>, Error<M::Error>> {
        self.get_timeout(self.0.config.get_timeout).await
    }

    pub async fn get_timeout(&self, timeout: Duration) -> Result<PooledObject<M>, Error<M::Error>> {
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
    ) -> Result<PooledObject<M>, Error<M::Error>> {
        let timeout = timeout(dur);
        let config = &self.0.config;

        let mut internals = self.0.internals.lock().await;
        let num_free = internals.free_conns.len();
        if strategy == GetStrategy::CachedOrNewConn && num_free > 0 {
            let c = internals.free_conns.swap_remove(0);
            if let Some(max_lifetime) = config.max_lifetime {
                drop(internals);
                if c.expired(max_lifetime) {
                    return Err(Error::BadConn);
                }

                let pooled = PooledObject {
                    pool: Some(self.clone()),
                    conn: Some(c),
                };
                return Ok(pooled);
            }
        }

        if let Some(max_open) = config.max_open {
            if internals.num_open >= max_open {
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
                                let pooled = PooledObject {
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
        match self.0.manager.create().await {
            Ok(c) => {
                let conn = Conn {
                    raw: Some(c),
                    last_err: Mutex::new(None),
                    created_at: Instant::now(),
                };
                let pooled = PooledObject {
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
}

async fn recycle_conn<M: Manager>(shared: &Arc<SharedPool<M>>, mut conn: Conn<M::Resource, M::Error>) {

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

    let create_r = shared.manager.create().await;
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
    mut internals: MutexGuard<'_, PoolInternals<M::Resource, M::Error>>,
    conn: Result<Conn<M::Resource, M::Error>, M::Error>,
) {
    if conn.is_err() {
        return maybe_open_new_connection(shared, internals).await;
    }

    let config = &shared.config;
    if let Some(max_open) = config.max_open {
        if internals.num_open > max_open {
            return;
        }
    }

    if internals.conn_requests.len() > 0 {
        let key = internals.conn_requests.keys().next().unwrap().clone();
        let req = internals.conn_requests.remove(&key).unwrap();

        // FIXME
        if let Err(Ok(conn)) = req.send(conn) {
            return put_free_conn(shared, internals, conn);
        }
        return;
    }

    if let Ok(conn) = conn {
        return put_free_conn(shared, internals, conn);
    }
}

fn put_free_conn<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Resource, M::Error>>,
    conn: Conn<M::Resource, M::Error>,
) {
    let config = &shared.config;
    match config.max_idle {
        Some(max_idle) if max_idle > internals.free_conns.len() as u64 => {
            internals.free_conns.push(conn)
        }
        None => internals.free_conns.push(conn),
        _ => (),
    }
}

async fn maybe_open_new_connection<M: Manager>(
    shared: &Arc<SharedPool<M>>,
    mut internals: MutexGuard<'_, PoolInternals<M::Resource, M::Error>>,
) {
    let mut num_requests = internals.conn_requests.len() as u64;
    let config = &shared.config;
    if let Some(max_open) = config.max_open {
        let num_can_open = max_open - internals.num_open;
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

async fn connection_cleaner<M: Manager>(shared: &Arc<SharedPool<M>>, max_lifetime: Duration) {
    let config = &shared.config;
    let mut interval = interval(config.clean_rate);
    let shared = Arc::downgrade(shared);

    loop {
        interval.tick().await;
        clean_connection(&shared, max_lifetime).await;
    }
}

async fn clean_connection<M: Manager>(shared: &Weak<SharedPool<M>>, max_lifetime: Duration) {
    let shared = match shared.upgrade() {
        Some(shared) => shared,
        None => return,
    };

    let config = &shared.config;

    let expired = Instant::now() - max_lifetime;
    let mut internals = shared.internals.lock().await;
    let mut closing = vec![];

    let mut i = 0;

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
    drop(closing);
}

pub struct PooledObject<M: Manager> {
    pool: Option<Pool<M>>,
    conn: Option<Conn<M::Resource, M::Error>>,
}

impl<M: Manager> Drop for PooledObject<M> {
    fn drop(&mut self) {
        let pool = self.pool.take().unwrap();
        let conn = self.conn.take().unwrap();
        spawn(pool.put_back_conn(conn));
    }
}

impl<M: Manager> Deref for PooledObject<M> {
    type Target = M::Resource;
    fn deref(&self) -> &Self::Target {
        &self.conn.as_ref().unwrap().raw.as_ref().unwrap()
    }
}

impl<M: Manager> DerefMut for PooledObject<M> {
    fn deref_mut(&mut self) -> &mut M::Resource {
        self.conn.as_mut().unwrap().raw.as_mut().unwrap()
    }
}
