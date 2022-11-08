use super::beatmap::Beatmap;

#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ReworkScore {
    pub score_id: i32,
    pub beatmap_id: i32,
    pub beatmapset_id: i32,
    pub user_id: i32,
    pub rework_id: i32,
    pub max_combo: i32,
    pub mods: i32,
    pub accuracy: f32,
    pub score: i64,
    pub num_300s: i32,
    pub num_100s: i32,
    pub num_50s: i32,
    pub num_gekis: i32,
    pub num_katus: i32,
    pub num_misses: i32,
    pub old_pp: f32,
    pub new_pp: f32,
}

impl ReworkScore {
    pub fn from_ripple_score(score: &RippleScore, rework_id: i32, new_pp: f32) -> Self {
        Self {
            score_id: score.id,
            beatmap_id: score.beatmap_id,
            beatmapset_id: score.beatmapset_id,
            user_id: score.userid,
            rework_id,
            max_combo: score.max_combo,
            mods: score.mods,
            accuracy: score.accuracy,
            score: score.score,
            num_300s: score.count_300,
            num_100s: score.count_100,
            num_50s: score.count_50,
            num_gekis: score.count_gekis,
            num_katus: score.count_katus,
            num_misses: score.count_misses,
            old_pp: score.pp,
            new_pp,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct APIBaseReworkScore {
    pub score_id: i32,
    pub beatmap_id: i32,
    pub beatmapset_id: i32,
    pub user_id: i32,
    pub rework_id: i32,
    pub max_combo: i32,
    pub mods: i32,
    pub accuracy: f32,
    pub score: i64,
    pub num_300s: i32,
    pub num_100s: i32,
    pub num_50s: i32,
    pub num_gekis: i32,
    pub num_katus: i32,
    pub num_misses: i32,
    pub old_pp: f32,
    pub new_pp: f32,
    pub old_rank: u64,
    pub new_rank: u64,
}

impl APIBaseReworkScore {
    pub fn from_score(score: ReworkScore, old_rank: u64, new_rank: u64) -> Self {
        Self {
            score_id: score.score_id,
            beatmap_id: score.beatmap_id,
            beatmapset_id: score.beatmapset_id,
            user_id: score.user_id,
            rework_id: score.rework_id,
            max_combo: score.max_combo,
            mods: score.mods,
            accuracy: score.accuracy,
            score: score.score,
            num_300s: score.num_300s,
            num_100s: score.num_100s,
            num_50s: score.num_50s,
            num_gekis: score.num_gekis,
            num_katus: score.num_katus,
            num_misses: score.num_misses,
            old_pp: score.old_pp,
            new_pp: score.new_pp,
            old_rank,
            new_rank,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct APIReworkScore {
    pub score_id: i32,
    pub user_id: i32,
    pub rework_id: i32,
    pub max_combo: i32,
    pub mods: i32,
    pub accuracy: f32,
    pub score: i64,
    pub num_300s: i32,
    pub num_100s: i32,
    pub num_50s: i32,
    pub num_gekis: i32,
    pub num_katus: i32,
    pub num_misses: i32,
    pub old_pp: f32,
    pub new_pp: f32,
    pub old_rank: u64,
    pub new_rank: u64,

    pub beatmap: Beatmap,
}

impl APIReworkScore {
    pub fn from_base(base: APIBaseReworkScore, beatmap: Beatmap) -> Self {
        Self {
            score_id: base.score_id,
            user_id: base.user_id,
            rework_id: base.rework_id,
            max_combo: base.max_combo,
            mods: base.mods,
            accuracy: base.accuracy,
            score: base.score,
            num_300s: base.num_300s,
            num_100s: base.num_100s,
            num_50s: base.num_50s,
            num_gekis: base.num_gekis,
            num_katus: base.num_katus,
            num_misses: base.num_misses,
            old_pp: base.old_pp,
            new_pp: base.new_pp,
            old_rank: base.old_rank,
            new_rank: base.new_rank,
            beatmap,
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct RippleScore {
    pub id: i32,
    pub beatmap_md5: String,
    pub userid: i32,
    pub score: i64,
    pub max_combo: i32,
    pub full_combo: bool,
    pub mods: i32,

    #[serde(rename = "300_count")]
    #[sqlx(rename = "300_count")]
    pub count_300: i32,

    #[serde(rename = "100_count")]
    #[sqlx(rename = "100_count")]
    pub count_100: i32,

    #[serde(rename = "50_count")]
    #[sqlx(rename = "50_count")]
    pub count_50: i32,

    #[serde(rename = "katus_count")]
    #[sqlx(rename = "katus_count")]
    pub count_katus: i32,

    #[serde(rename = "gekis_count")]
    #[sqlx(rename = "gekis_count")]
    pub count_gekis: i32,

    #[serde(rename = "misses_count")]
    #[sqlx(rename = "misses_count")]
    pub count_misses: i32,

    pub time: i64,
    pub play_mode: i32,
    pub completed: i32,
    pub accuracy: f32,
    pub pp: f32,
    pub checksum: Option<String>,
    pub patcher: bool,
    pub pinned: bool,

    pub beatmap_id: i32,
    pub beatmapset_id: i32,
}
