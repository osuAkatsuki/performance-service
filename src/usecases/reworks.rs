use crate::{context::Context, models::rework::Rework, repositories};
use std::sync::Arc;

pub async fn fetch_one(rework_id: i32, context: Arc<Context>) -> anyhow::Result<Option<Rework>> {
    let repo = repositories::reworks::ReworksRepository::new(context);
    let rework = repo.fetch_one(rework_id).await?;

    Ok(rework)
}

pub async fn fetch_all(context: Arc<Context>) -> anyhow::Result<Vec<Rework>> {
    let repo = repositories::reworks::ReworksRepository::new(context);
    let reworks = repo.fetch_all().await?;

    Ok(reworks)
}
