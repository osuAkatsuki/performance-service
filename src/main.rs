use clap::Parser;
use deadpool_lapin::{Manager, Pool};
use lapin::ConnectionProperties;
use performance_service::{
    api, config::Config, context::Context, deploy, individual_recalc, mass_recalc,
    models::pool::DbPool, processor,
};
use redis::{Client, ConnectionAddr, ConnectionInfo, RedisConnectionInfo};
use s3::{creds::Credentials, Bucket, Region};
use sqlx::{mysql::MySqlConnectOptions, ConnectOptions};
use structured_logger::{async_json::new_writer, Builder};

fn amqp_dsn(username: &str, password: &str, host: &str, port: u16) -> String {
    return format!("amqp://{}:{}@{}:{}", username, password, host, port);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    Builder::new()
        .with_target_writer("*", new_writer(tokio::io::stdout()))
        .init();

    let config = Config::parse();

    let database_options = MySqlConnectOptions::new()
        .host(&config.database_host)
        .port(config.database_port)
        .username(&config.database_username)
        .password(&config.database_password)
        .database(&config.database_name)
        .disable_statement_logging()
        .clone();
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
        addr: match config.redis_use_ssl {
            true => ConnectionAddr::TcpTls {
                host: config.redis_host.clone(),
                port: config.redis_port,
                insecure: false,
            },
            false => ConnectionAddr::Tcp(config.redis_host.clone(), config.redis_port),
        },
        redis: RedisConnectionInfo {
            db: config.redis_database,
            password: config.redis_password.clone(),
            username: config.redis_username.clone(),
        },
    };
    let redis = Client::open(redis_connection_options)?;

    let custom_region = Region::Custom {
        region: config.aws_region.clone(),
        endpoint: config.aws_endpoint_url.clone(),
    };
    let bucket = Bucket::new(
        &config.aws_bucket_name,
        custom_region,
        Credentials {
            access_key: Some(config.aws_access_key_id.clone()),
            secret_key: Some(config.aws_secret_access_key.clone()),
            security_token: None,
            session_token: None,
            expiration: None,
        },
    )?;

    let context = Context {
        config,
        database,
        amqp_channel,
        redis,
        bucket,
    };

    match context.config.app_component.as_str() {
        "api" => api::serve(context).await?,
        "processor" => processor::serve(context).await?,
        "mass_recalc" => mass_recalc::serve(context).await?,
        "deploy" => deploy::serve(context).await?,
        "individual_recalc" => individual_recalc::serve(context).await?,
        _ => panic!("unknown app component"),
    }

    Ok(())
}
