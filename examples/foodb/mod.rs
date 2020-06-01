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
