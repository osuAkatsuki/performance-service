use clap::Parser;
use deadpool_lapin::{Manager, Pool};
use lapin::ConnectionProperties;
use performance_service::{api, config::Config, context::Context, deploy, mass_recalc, processor};
use redis::Client;
use sqlx::mysql::MySqlPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let config = Config::parse();

    let database = MySqlPoolOptions::new()
        .connect(&config.database_url)
        .await?;

    let amqp_manager = Manager::new(config.amqp_url.clone(), ConnectionProperties::default());
    let amqp = Pool::builder(amqp_manager).max_size(10).build()?;
    let amqp_channel = amqp.get().await?.create_channel().await?;

    let redis = Client::open(config.redis_url.clone())?;

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
