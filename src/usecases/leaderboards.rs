use crate::{
    context::Context,
    errors::{Error, ErrorCode},
    models::leaderboard::Leaderboard,
    repositories::leaderboards::LeaderboardsRepository,
};
use std::sync::Arc;

pub async fn fetch_one(
    rework_id: i32,
    offset: i32,
    limit: i32,
    context: Arc<Context>,
) -> Result<Leaderboard, Error> {
    let repo = LeaderboardsRepository::new(context);
    let rework = repo
        .fetch_one(rework_id, offset, limit)
        .await
        .map_err(|_| Error {
            error_code: ErrorCode::InternalServerError,
            user_feedback: "Failed to fetch leaderboard",
        })?;

    match rework {
        Some(rework) => Ok(rework),
        None => Err(Error {
            error_code: ErrorCode::NotFound,
            user_feedback: "Leaderboard not found",
        }),
    }
}
