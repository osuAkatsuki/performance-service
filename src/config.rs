#[derive(clap::Parser, Clone)]
pub struct Config {
    #[clap(long, env)]
    pub app_component: String,

    #[clap(long, env)]
    pub api_port: Option<u16>,

    #[clap(long, env)]
    pub database_url: String,

    #[clap(long, env)]
    pub amqp_url: String,

    #[clap(long, env)]
    pub redis_url: String,

    #[clap(long, env)]
    pub beatmaps_path: String,
}
