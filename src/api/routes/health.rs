use axum::{extract::Extension, http, routing::get, Router};
use redis::{aio::ConnectionLike, Cmd};
use sqlx::MySql;
use std::{ops::DerefMut, sync::Arc};

use crate::context::Context;

pub fn router() -> Router {
    Router::new().route("/_health", get(health))
}

async fn health(Extension(ctx): Extension<Arc<Context>>) -> http::StatusCode {
    let mut is_redis_ok = false;
    let mut is_database_ok = false;

    if let Ok(mut conn) = ctx.redis.get_multiplexed_async_connection().await {
        if let Ok(_result) = conn
            .req_packed_command(&Cmd::new().arg("PING").arg(1))
            .await
        {
            is_redis_ok = true;
        }
    }

    if let Ok(_result) = sqlx::query_scalar::<MySql, i32>("SELECT 1")
        .fetch_one(ctx.database.get().await.unwrap().deref_mut())
        .await
    {
        is_database_ok = true;
    }

    let is_ok = is_redis_ok && is_database_ok;
    match is_ok {
        true => http::StatusCode::OK,
        false => {
            log::error!(
                redis_healthy = is_redis_ok,
                database_healthy = is_database_ok;
                "Failed health check.",
            );
            return http::StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
}
