use async_trait::async_trait;
use deadpool::managed::{Manager, RecycleResult};
use sqlx::{Connection, Error as SqlxError, MySqlConnection};

#[derive(Clone)]
pub struct DbPool {
    url: String,
}

#[async_trait]
impl Manager for DbPool {
    type Type = MySqlConnection;
    type Error = SqlxError;

    async fn create(&self) -> Result<MySqlConnection, SqlxError> {
        MySqlConnection::connect(&self.url).await
    }

    async fn recycle(&self, obj: &mut MySqlConnection) -> RecycleResult<SqlxError> {
        Ok(obj.ping().await?)
    }
}

type Pool = deadpool::managed::Pool<DbPool>;

impl DbPool {
    pub fn new(url: String) -> Pool {
        Pool::builder(Self { url }).max_size(16).build().unwrap()
    }
}
