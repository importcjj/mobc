mod event;
mod spawn;

use futures::select;
use futures::StreamExt;
use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures_channel::oneshot::{channel, Sender};

pub use async_trait::async_trait;
pub use spawn::spawn;

use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::Drop;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[async_trait]
pub trait Manager: Send + Sync + 'static {
    type Connection: Send + Sync + 'static;
    type Error: Debug + Send + Sync + 'static;

    async fn connect(&self) -> std::result::Result<Self::Connection, Self::Error>;
}

#[async_trait]
pub trait Checker: Send {
    type Connection: Send + Sync + 'static;
    type Error: Debug + Send + Sync + 'static;

    async fn check(&self, conn: &Self::Connection) -> std::result::Result<(), Self::Error>;
}

pub struct DummyChecker<C, E> {
    c: PhantomData<C>,
    e: PhantomData<E>,
}

impl<C, E> DummyChecker<C, E> {
    fn new() -> Self {
        Self {
            c: PhantomData,
            e: PhantomData,
        }
    }
}

#[async_trait]
impl<C, E> Checker for DummyChecker<C, E>
where
    C: Sync + Send + 'static,
    E: Debug + Send + Sync + 'static,
{
    type Connection = C;
    type Error = E;

    async fn check(&self, _: &C) -> std::result::Result<(), E> {
        Ok(())
    }
}

struct PoolBackend<M: Manager> {
    requests: UnboundedReceiver<GetRequest<M>>,
    idles: Vec<M::Connection>,
    pendings: Vec<GetRequest<M>>,

    num_open: i64,
    max_open: i64,
}

impl<M: Manager> PoolBackend<M> {
    fn new(max_open: i64, requests: UnboundedReceiver<GetRequest<M>>) -> Self {
        Self {
            requests,
            idles: Vec::new(),
            pendings: Vec::new(),

            num_open: 0,
            max_open,
        }
    }

    async fn run(mut self, manager: M) -> Result<()> {
        let mut requests = self.requests.fuse();
        let (recycle_tx, recycle_rx) = unbounded::<M::Connection>();
        let mut recycle = recycle_rx.fuse();
        let (open_tx, open_rx) = unbounded::<()>();

        let new_conns = recycle_tx.clone();
        spawn::spawn(async move {
            let worker = Worker::new(manager);
            worker.run(open_rx, new_conns).await
        });

        macro_rules! respond_req {
            () => {
                while self.idles.len() > 0 && self.pendings.len() > 0 {
                    let pending = self.pendings.swap_remove(0);
                    let idle = self.idles.swap_remove(0);
                    let conn = Conn::<M>::new(idle, recycle_tx.clone());
                    pending.respond(conn);
                }
            };
        }

        macro_rules! maybe_open_conn {
            () => {
                let mut can_open = self.max_open - self.num_open;
                let num_pendings = self.pendings.len() as i64;
                if num_pendings < can_open {
                    can_open = num_pendings;
                }

                while can_open > 0 {
                    open_tx.unbounded_send(()).ok();
                    can_open -= 1;
                    self.num_open += 1;
                }
            };
        }

        'event_loop: loop {
            respond_req!();

            select! {
                req = requests.next() => {
                    match req {
                        Some(req) => {
                            self.pendings.push(req);
                            maybe_open_conn!();
                        }
                        None => {break 'event_loop}
                    }

                },
                conn = recycle.next() => {
                    let raw = conn.unwrap();
                    self.idles.push(raw);
                    respond_req!();
                },
            }
        }

        Ok(())
    }
}

struct Worker<M: Manager> {
    manager: M,
}

impl<M: Manager> Worker<M> {
    fn new(manager: M) -> Self {
        Self { manager }
    }

    async fn run(self, mut ch: UnboundedReceiver<()>, tx: UnboundedSender<M::Connection>) {
        while let Some(_) = ch.next().await {
            let raw = self.manager.connect().await.unwrap();
            tx.unbounded_send(raw).unwrap();
        }
    }
}

pub struct Pool<
    M: Manager,
    C: Checker = DummyChecker<<M as Manager>::Connection, <M as Manager>::Error>,
> {
    tx: UnboundedSender<GetRequest<M>>,
    checker: Arc<C>,
}

impl<M: Manager> Pool<M> {
    pub fn new(manager: M, max_open: i64) -> Self {
        let (req_tx, req_rx) = unbounded();
        let backend = PoolBackend::new(max_open, req_rx);

        spawn::spawn(async move { backend.run(manager).await });

        Self {
            tx: req_tx,
            checker: Arc::new(DummyChecker::new()),
        }
    }
}

impl<M: Manager, C> Pool<M, C>
where
    C: Checker<Connection = M::Connection, Error = M::Error>,
{
    pub async fn get(&self) -> Result<Conn<M>> {
        let (tx, rx) = channel();
        let req = GetRequest::new(tx);
        // Fix me
        self.tx.unbounded_send(req).ok();

        let conn = rx.await?;

        self.checker
            .check(&conn.raw.as_ref().unwrap())
            .await
            .unwrap();

        Ok(conn)
    }
}

impl<M: Manager, C: Checker> Clone for Pool<M, C> {
    fn clone(&self) -> Self {
        Pool {
            tx: self.tx.clone(),
            checker: self.checker.clone(),
        }
    }
}

struct GetRequest<M: Manager> {
    tx: Sender<Conn<M>>,
}

impl<M: Manager> GetRequest<M> {
    fn new(tx: Sender<Conn<M>>) -> Self {
        Self { tx }
    }

    fn respond(self, conn: Conn<M>) {
        self.tx.send(conn).ok();
    }
}

#[derive(Debug)]
pub struct Conn<M: Manager> {
    raw: Option<M::Connection>,
    recycle_ch: UnboundedSender<M::Connection>,
}

impl<M: Manager> Conn<M> {
    fn new(raw: M::Connection, recycle_ch: UnboundedSender<M::Connection>) -> Self {
        Self {
            raw: Some(raw),
            recycle_ch,
        }
    }
}

impl<M: Manager> Drop for Conn<M> {
    fn drop(&mut self) {
        if let Some(raw) = self.raw.take() {
            self.recycle_ch.unbounded_send(raw).unwrap();
        }
    }
}

impl<M: Manager> Deref for Conn<M> {
    type Target = M::Connection;
    fn deref(&self) -> &Self::Target {
        &self.raw.as_ref().unwrap()
    }
}

impl<M: Manager> DerefMut for Conn<M> {
    fn deref_mut(&mut self) -> &mut M::Connection {
        self.raw.as_mut().unwrap()
    }
}
