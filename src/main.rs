use clap::Parser;
use deadpool_lapin::{Manager, Pool};
use lapin::ConnectionProperties;
use performance_service::{
    api, config::Config, context::Context, deploy, mass_recalc, models::pool::DbPool, processor,
};
use redis::{Client, ConnectionAddr, ConnectionInfo, RedisConnectionInfo};
use sqlx::mysql::MySqlConnectOptions;

fn amqp_dsn(username: &str, password: &str, host: &str, port: u16) -> String {
    return format!("amqp://{}:{}@{}:{}", username, password, host, port);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let config = Config::parse();

    let database_options = MySqlConnectOptions::new()
        .host(&config.database_host)
        .port(config.database_port)
        .username(&config.database_username)
        .password(&config.database_password)
        .database(&config.database_name);
    let database = DbPool::new(database_options, config.database_pool_max_size)?;

    let amqp_url = amqp_dsn(
        &config.amqp_username,
        &config.amqp_password,
        &config.amqp_host,
        config.amqp_port,
    );
    let amqp_manager = Manager::new(amqp_url, ConnectionProperties::default());
    let amqp = Pool::builder(amqp_manager)
        .max_size(config.amqp_pool_max_size)
        .build()?;
    let amqp_channel = amqp.get().await?.create_channel().await?;

    let redis_connection_options = ConnectionInfo {
        addr: ConnectionAddr::Tcp(config.redis_host.clone(), config.redis_port),
        redis: RedisConnectionInfo {
            db: config.redis_database,
            password: config.redis_password.clone(),
            username: config.redis_username.clone(),
        },
    };
    let redis = Client::open(redis_connection_options)?;

    let context = Context {
        config,
        database,
        amqp_channel,
        redis,
    };

    match context.config.app_component.as_str() {
        "api" => api::serve(context).await?,
        "processor" => processor::serve(context).await?,
        "mass_recalc" => mass_recalc::serve(context).await?,
        "deploy" => deploy::serve(context).await?,
        _ => panic!("unknown app component"),
    }

    Ok(())
}
