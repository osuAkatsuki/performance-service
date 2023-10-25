use deadpool::managed::Pool;
use lapin::Channel;
use redis::Client;
use s3::Bucket;

use crate::{config::Config, models::pool::DbPool};

#[derive(Clone)]
pub struct Context {
    pub config: Config,
    pub database: Pool<DbPool>,
    pub amqp_channel: Channel,
    pub redis: Client,
    pub bucket: Bucket,
}
