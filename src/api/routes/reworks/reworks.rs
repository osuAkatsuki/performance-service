use std::sync::Arc;

use axum::{
    extract::{Extension, Path},
    routing::get,
    Json, Router,
};

use crate::{api::error::AppResult, context::Context, models::rework::Rework, usecases};

pub fn router() -> Router {
    Router::new()
        .route("/api/v1/reworks", get(get_reworks))
        .route("/api/v1/reworks/:rework_id", get(get_rework))
}

async fn get_reworks(Extension(ctx): Extension<Arc<Context>>) -> AppResult<Json<Vec<Rework>>> {
    let reworks = usecases::reworks::fetch_all(ctx.clone()).await?;
    Ok(Json(reworks))
}

async fn get_rework(
    Extension(ctx): Extension<Arc<Context>>,
    Path(rework_id): Path<i32>,
) -> AppResult<Json<Option<Rework>>> {
    let rework = usecases::reworks::fetch_one(rework_id, ctx.clone()).await?;
    Ok(Json(rework))
}
