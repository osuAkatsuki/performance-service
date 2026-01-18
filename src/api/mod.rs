use std::sync::Arc;

use axum::{AddExtensionLayer, Router};
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::context::Context;

mod error;
mod routes;

fn api_router() -> Router {
    routes::calculate::router()
        .merge(routes::reworks::queue::router())
        .merge(routes::reworks::scores::router())
        .merge(routes::reworks::reworks::router())
        .merge(routes::reworks::user::router())
        .merge(routes::reworks::leaderboard::router())
        .merge(routes::reworks::sessions::router())
        .merge(routes::reworks::search::router())
        .merge(routes::health::router())
}

pub async fn serve(ctx: Context) -> anyhow::Result<()> {
    let server_port = ctx.config.api_port.unwrap();
    let server_host = ctx.config.api_host.as_ref().unwrap().clone();

    let app = api_router().layer(
        ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(AddExtensionLayer::new(Arc::new(ctx))),
    );

    log::info!(
        port = server_port;
        "Serving API",
    );
    axum::Server::bind(&format!("{}:{}", server_host, server_port).parse()?)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
