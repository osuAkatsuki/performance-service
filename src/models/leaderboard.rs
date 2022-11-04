use super::stats::APIReworkStats;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Leaderboard {
    pub total_count: i32,
    pub users: Vec<APIReworkStats>,
}
