#[derive(clap::Parser, Clone)]
pub struct Config {
    #[clap(long, env)]
    pub app_component: String,

    #[clap(long, env)]
    pub api_port: Option<u16>,

    #[clap(long, env)]
    pub database_host: String,

    #[clap(long, env)]
    pub database_port: u16,

    #[clap(long, env)]
    pub database_username: String,

    #[clap(long, env)]
    pub database_password: String,

    #[clap(long, env)]
    pub database_name: String,

    #[clap(long, env)]
    pub amqp_host: String,

    #[clap(long, env)]
    pub amqp_port: u16,

    #[clap(long, env)]
    pub amqp_username: String,

    #[clap(long, env)]
    pub amqp_password: String,

    #[clap(long, env)]
    pub redis_host: String,

    #[clap(long, env)]
    pub redis_port: u16,

    #[clap(long, env)]
    pub redis_username: Option<String>,

    #[clap(long, env)]
    pub redis_password: Option<String>,

    #[clap(long, env)]
    pub redis_database: i64,

    #[clap(long, env)]
    pub beatmaps_path: String,
}
