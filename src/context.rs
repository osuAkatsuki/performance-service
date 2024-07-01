use lapin::Channel;
use redis::Client;
use s3::Bucket;
use sqlx::MySqlPool;

use crate::config::Config;

#[derive(Clone)]
pub struct Context {
    pub config: Config,
    pub database: MySqlPool,
    pub amqp_channel: Channel,
    pub redis: Client,
    pub bucket: Bucket,
}
