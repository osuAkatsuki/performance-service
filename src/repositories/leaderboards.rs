use crate::{
    context::Context, models::leaderboard::Leaderboard, models::rework::Rework,
    models::stats::APIReworkStats,
};
use std::{ops::DerefMut, sync::Arc};

pub struct LeaderboardsRepository {
    context: Arc<Context>,
}

impl LeaderboardsRepository {
    pub fn new(context: Arc<Context>) -> Self {
        Self { context }
    }

    pub async fn fetch_one(
        &self,
        rework_id: i32,
        offset: i32,
        limit: i32,
    ) -> anyhow::Result<Option<Leaderboard>> {
        let rework: Rework = match sqlx::query_as(r#"SELECT * FROM reworks WHERE rework_id = ?"#)
            .bind(rework_id)
            .fetch_optional(self.context.database.get().await?.deref_mut())
            .await?
        {
            Some(rework) => rework,
            None => return Ok(None),
        };

        let leaderboard_count: i32 =
            sqlx::query_scalar("SELECT COUNT(*) FROM rework_stats WHERE rework_id = ?")
                .bind(rework.rework_id)
                .fetch_one(self.context.database.get().await?.deref_mut())
                .await
                .unwrap();

        let rework_users: Vec<APIReworkStats> = sqlx::query_as(
            "SELECT user_id, users_stats.country, users.username user_name, rework_id, old_pp, new_pp, 
            DENSE_RANK() OVER (ORDER BY old_pp DESC) old_rank, DENSE_RANK() OVER (ORDER BY new_pp DESC) new_rank 
            FROM 
                rework_stats 
            INNER JOIN 
                users_stats
                ON users_stats.id = rework_stats.user_id
            INNER JOIN
                users
                ON users.id = rework_stats.user_id
            WHERE 
                rework_id = ?
            ORDER BY 
                new_pp DESC
            LIMIT ?, ?"
        )
            .bind(rework.rework_id)
            .bind(offset)
            .bind(limit)
            .fetch_all(self.context.database.get().await?.deref_mut())
            .await
            .unwrap();

        let leaderboard = Leaderboard {
            total_count: leaderboard_count,
            users: rework_users,
        };

        Ok(Some(leaderboard))
    }
}
