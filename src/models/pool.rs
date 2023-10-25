use async_trait::async_trait;
use deadpool::managed::{Manager, RecycleResult};
use sqlx::{
    mysql::MySqlConnectOptions, ConnectOptions, Connection, Error as SqlxError, MySqlConnection,
};

#[derive(Clone)]
pub struct DbPool {
    options: MySqlConnectOptions,
}

#[async_trait]
impl Manager for DbPool {
    type Type = MySqlConnection;
    type Error = SqlxError;

    async fn create(&self) -> Result<MySqlConnection, SqlxError> {
        self.options.connect().await
    }

    async fn recycle(&self, obj: &mut MySqlConnection) -> RecycleResult<SqlxError> {
        Ok(obj.ping().await?)
    }
}

type Pool = deadpool::managed::Pool<DbPool>;

impl DbPool {
    pub fn new(options: MySqlConnectOptions, max_size: usize) -> anyhow::Result<Pool> {
        Ok(Pool::builder(Self { options }).max_size(max_size).build()?)
    }
}
