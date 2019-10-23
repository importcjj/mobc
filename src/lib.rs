use futures::Future;
use std::marker::Unpin;
use std::ops::Deref;


#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    User(E),
    Void,
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Self::User(e)
    }
}

type AnyFuture<T, E> = Box<dyn Future<Output = Result<T, E>> + Unpin>;

pub trait ConnectionManager {
    type Connection: Send + 'static;
    type Error: std::error::Error + 'static;

    fn connect(&self) -> AnyFuture<Self::Connection, Self::Error>;
    fn is_valid(&self, conn: &mut Self::Connection) -> AnyFuture<(), Self::Error>;
    fn has_broken(&self, conn: &mut Self::Connection) -> bool;
}

pub struct Pool<M>
where
    M: ConnectionManager,
{
    manager: M,
}

impl<M> Pool<M>
where
    M: ConnectionManager,
{
    async fn new<E>(manager: M) -> Result<Pool<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        manager.connect().await?;
        Ok(Pool { manager })
    }

    async fn get<E>(&self) -> Result<PooledConnection<M>, Error<E>>
    where
        Error<E>: std::convert::From<<M as ConnectionManager>::Error>,
    {
        self.manager.connect().await
    }
}

pub struct PooledConnection<M> 
where
    M: ConnectionManager
{
    pool: Pool<M>,
    conn: M::Connection,
}

impl<M> Deref for PooledConnection<M> 
where
    M: ConnectionManager
{
    type Target = M::Connection;
    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}