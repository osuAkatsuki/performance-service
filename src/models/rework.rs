#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct Rework {
    pub rework_id: i32,
    pub rework_name: String,
    pub mode: i32,
    pub rx: i32,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
