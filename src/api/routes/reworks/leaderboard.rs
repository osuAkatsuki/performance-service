use std::sync::Arc;

use axum::{
    extract::{Extension, Path, Query},
    routing::get,
    Json, Router,
};

use crate::{context::Context, models::leaderboard::Leaderboard, usecases};

pub fn router() -> Router {
    Router::new().route(
        "/api/v1/reworks/:rework_id/leaderboards",
        get(get_rework_leaderboards),
    )
}

#[derive(serde::Serialize, serde::Deserialize)]
struct LeaderboardQuery {
    page: i32,
    amount: i32,
}

async fn get_rework_leaderboards(
    Extension(ctx): Extension<Arc<Context>>,
    Path(rework_id): Path<i32>,
    Query(query): Query<LeaderboardQuery>,
) -> Json<Option<Leaderboard>> {
    let leaderboard = usecases::leaderboards::fetch_one(
        rework_id,
        (query.page.max(1) - 1) * query.amount,
        query.amount,
        ctx.clone(),
    )
    .await
    .unwrap();

    Json(leaderboard)
}
