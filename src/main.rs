use clap::Parser;
use deadpool_lapin::{Manager, Pool};
use lapin::ConnectionProperties;
use performance_service::{
    api, config::Config, context::Context, deploy, mass_recalc, models::pool::DbPool, processor,
};
use redis::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let config = Config::parse();

    let database = DbPool::new(config.database_url.clone());

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
