use crate::{context::Context, models::leaderboard::Leaderboard, repositories};
use std::sync::Arc;

pub async fn fetch_one(
    rework_id: i32,
    offset: i32,
    limit: i32,
    context: Arc<Context>,
) -> anyhow::Result<Option<Leaderboard>> {
    let repo = repositories::leaderboards::LeaderboardsRepository::new(context);
    let rework = repo.fetch_one(rework_id, offset, limit).await?;

    Ok(rework)
}
