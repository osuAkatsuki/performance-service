use super::rework::Rework;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ReworkUser {
    pub user_id: i32,
    pub user_name: String,
    pub country: String,
    pub reworks: Vec<Rework>,
}
