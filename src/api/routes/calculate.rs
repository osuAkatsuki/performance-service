use crate::context::Context;
use akatsuki_pp_rs::{Beatmap, BeatmapExt, GameMode};
use axum::{extract::Extension, routing::post, Json, Router};
use oppai_rs::{Combo, Mods as OppaiMods, Oppai};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn router() -> Router {
    Router::new().route("/api/v1/calculate", post(calculate_play))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CalculateRequest {
    pub beatmap_id: i32,
    pub mode: i32,
    pub mods: i32,
    pub max_combo: i32,
    pub accuracy: f32,
    pub miss_count: i32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CalculateResponse {
    pub stars: f32,
    pub pp: f32,
}

fn round(x: f32, decimals: u32) -> f32 {
    let y = 10i32.pow(decimals) as f32;
    (x * y).round() / y
}

async fn calculate_oppai_pp(
    beatmap_path: PathBuf,
    request: &CalculateRequest,
) -> CalculateResponse {
    let mut oppai: &mut Oppai = &mut Oppai::new(&beatmap_path).unwrap();

    oppai = oppai
        .mods(OppaiMods::from_bits_truncate(request.mods))
        .combo(Combo::NonFC {
            max_combo: request.max_combo as u32,
            misses: request.miss_count as u32,
        })
        .unwrap()
        .accuracy(request.accuracy)
        .unwrap();

    let (mut pp, mut stars) = oppai.run();
    pp = round(pp, 2);
    stars = round(stars, 2);

    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    if stars.is_infinite() || stars.is_nan() {
        stars = 0.0;
    }

    CalculateResponse { stars, pp }
}

async fn calculate_bancho_pp(
    beatmap_path: PathBuf,
    request: &CalculateRequest,
) -> CalculateResponse {
    let beatmap = match Beatmap::from_path(beatmap_path).await {
        Ok(beatmap) => beatmap,
        Err(_) => {
            return CalculateResponse {
                stars: 0.0,
                pp: 0.0,
            }
        }
    };

    let result = beatmap
        .pp()
        .mode(match request.mode {
            0 => GameMode::Osu,
            1 => GameMode::Taiko,
            2 => GameMode::Catch,
            3 => GameMode::Mania,
            _ => unreachable!(),
        })
        .mods(request.mods as u32)
        .combo(request.max_combo as usize)
        .accuracy(request.accuracy as f64)
        .n_misses(request.miss_count as usize)
        .calculate();

    let mut pp = round(result.pp() as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    let mut stars = round(result.stars() as f32, 2);
    if stars.is_infinite() || stars.is_nan() {
        stars = 0.0;
    }

    CalculateResponse { stars, pp }
}

const RX: i32 = 1 << 7;
const AP: i32 = 1 << 13;

async fn calculate_play(
    Extension(ctx): Extension<Arc<Context>>,
    Json(requests): Json<Vec<CalculateRequest>>,
) -> Json<Vec<CalculateResponse>> {
    let mut results = Vec::new();

    for request in requests {
        let beatmap_path =
            Path::new(&ctx.config.beatmaps_path).join(format!("{}.osu", request.beatmap_id));

        let result = if request.mods & RX > 0 || request.mods & AP > 0 {
            calculate_oppai_pp(beatmap_path, &request).await
        } else {
            calculate_bancho_pp(beatmap_path, &request).await
        };

        results.push(result);
    }

    Json(results)
}
