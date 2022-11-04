#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct Beatmap {
    pub beatmap_id: i32,
    pub beatmapset_id: i32,
    pub song_name: String,
}
