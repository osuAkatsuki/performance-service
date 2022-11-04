#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ReworkStats {
    pub user_id: i32,
    pub rework_id: i32,
    pub old_pp: i32,
    pub new_pp: i32,
}

#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct APIReworkStats {
    pub user_id: i32,
    pub country: String,
    pub user_name: String,
    pub new_pp: i32,
    pub old_pp: i32,
    pub new_rank: u64,
    pub old_rank: u64,
}

impl APIReworkStats {
    pub fn from_stats(
        stats: ReworkStats,
        country: String,
        username: String,
        old_rank: u64,
        new_rank: u64,
    ) -> Self {
        Self {
            user_id: stats.user_id,
            country,
            user_name: username,
            new_pp: stats.new_pp,
            old_pp: stats.old_pp,
            new_rank,
            old_rank,
        }
    }
}
