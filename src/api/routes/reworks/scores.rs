use std::{ops::DerefMut, sync::Arc};

use axum::{
    extract::{Extension, Path},
    routing::get,
    Json, Router,
};

use crate::{
    context::Context,
    models::{
        beatmap::Beatmap,
        score::{APIBaseReworkScore, APIReworkScore},
    },
};

pub fn router() -> Router {
    Router::new().route(
        "/api/v1/reworks/:rework_id/scores/:user_id",
        get(get_rework_scores),
    )
}

async fn get_rework_scores(
    ctx: Extension<Arc<Context>>,
    Path((rework_id, user_id)): Path<(i32, i32)>,
) -> Json<Option<Vec<APIReworkScore>>> {
    let base_scores: Vec<APIBaseReworkScore> =
        sqlx::query_as(
            "SELECT user_id, rework_scores.beatmap_id, rework_scores.beatmapset_id, beatmap.song_name, rework_id, score_id, rework_scores.max_combo, mods, accuracy, score, num_300s, num_100s, num_50s, num_gekis,
            num_katus, num_misses, old_pp, new_pp,
            DENSE_RANK() OVER (ORDER BY old_pp DESC) old_rank, DENSE_RANK() OVER (ORDER BY new_pp DESC) new_rank
            FROM
                rework_scores
            INNER JOIN beatmaps
                ON rework_scores.beatmap_id = beatmaps.beatmap_id
            WHERE
                user_id = ? AND rework_id = ?
            ORDER BY
                new_pp DESC
            LIMIT 100",
        )
            .bind(user_id)
            .bind(rework_id)
            .fetch_all(ctx.database.get().await.unwrap().deref_mut())
            .await
            .unwrap();

    let mut scores: Vec<APIReworkScore> = Vec::new();
    for base_score in base_scores {
        let beatmap = Beatmap {
            beatmap_id: base_score.beatmap_id,
            beatmapset_id: base_score.beatmapset_id,
            song_name: base_score.song_name,
        };

        let score = APIReworkScore::from_base(base_score, beatmap);
        scores.push(score);
    }

    match scores.is_empty() {
        true => Json(None),
        false => Json(Some(scores)),
    }
}
