use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query},
    routing::post,
    Json, Router,
};

use crate::{api::error::AppResult, context::Context, models::queue::QueueResponse, usecases};

pub fn router() -> Router {
    Router::new().route("/api/v1/reworks/:rework_id/queue", post(send_to_queue))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct QueueRequestQuery {
    session: String,
}

async fn send_to_queue(
    Extension(ctx): Extension<Arc<Context>>,
    Path(rework_id): Path<i32>,
    Query(query): Query<QueueRequestQuery>,
) -> AppResult<Json<QueueResponse>> {
    let response = usecases::sessions::enqueue(query.session, rework_id, ctx.clone()).await?;

    Ok(Json(response))
}
