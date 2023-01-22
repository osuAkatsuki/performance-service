use crate::context::Context;
use crate::models::rework::Rework;
use std::ops::DerefMut;
use std::sync::Arc;

pub struct ReworksRepository {
    context: Arc<Context>,
}

impl ReworksRepository {
    pub fn new(context: Arc<Context>) -> Self {
        Self { context }
    }

    pub async fn fetch_one(&self, rework_id: i32) -> anyhow::Result<Option<Rework>> {
        let rework: Option<Rework> = sqlx::query_as(r#"SELECT * FROM reworks WHERE rework_id = ?"#)
            .bind(rework_id)
            .fetch_optional(self.context.database.get().await?.deref_mut())
            .await?;

        Ok(rework)
    }

    pub async fn fetch_all(&self) -> anyhow::Result<Vec<Rework>> {
        let reworks: Vec<Rework> = sqlx::query_as(r#"SELECT * FROM reworks"#)
            .fetch_all(self.context.database.get().await?.deref_mut())
            .await?;

        Ok(reworks)
    }
}
