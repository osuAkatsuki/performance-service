use crate::context::Context;
use crate::usecases;
use akatsuki_pp_rs::{Beatmap, BeatmapExt, GameMode, PerformanceAttributes};
use axum::{extract::Extension, routing::post, Json, Router};
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
    pub ar: f32,
    pub od: f32,
    pub max_combo: i32,
}

fn round(x: f32, decimals: u32) -> f32 {
    let y = 10i32.pow(decimals) as f32;
    (x * y).round() / y
}

async fn calculate_relax_pp(
    request: &CalculateRequest,
    context: Arc<Context>,
) -> anyhow::Result<CalculateResponse> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(request.beatmap_id, context).await?;
    let beatmap = Beatmap::from_bytes(&beatmap_bytes).await?;

    let result = akatsuki_pp_rs::osu_2019::OsuPP::new(&beatmap)
        .mods(request.mods as u32)
        .combo(request.max_combo as usize)
        .misses(request.miss_count as usize)
        .accuracy(request.accuracy)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        log::warn!("Calculated pp is infinite or NaN, setting to 0");
        pp = 0.0;
    }

    let mut stars = round(result.difficulty.stars as f32, 2);
    if stars.is_infinite() || stars.is_nan() {
        log::warn!("Calculated star rating is infinite or NaN, setting to 0");
        stars = 0.0;
    }

    Ok(CalculateResponse {
        stars,
        pp,
        ar: result.difficulty.ar as f32,
        od: result.difficulty.od as f32,
        max_combo: result.difficulty.max_combo as i32,
    })
}

async fn calculate_rosu_pp(
    request: &CalculateRequest,
    context: Arc<Context>,
) -> anyhow::Result<CalculateResponse> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(request.beatmap_id, context).await?;
    let beatmap = Beatmap::from_bytes(&beatmap_bytes).await?;

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
        log::warn!("Calculated pp is infinite or NaN, setting to 0");
        pp = 0.0;
    }

    let mut stars = round(result.stars() as f32, 2);
    if stars.is_infinite() || stars.is_nan() {
        log::warn!("Calculated star rating is infinite or NaN, setting to 0");
        stars = 0.0;
    }

    Ok(match result {
        PerformanceAttributes::Osu(result) => CalculateResponse {
            stars,
            pp,
            ar: result.difficulty.ar as f32,
            od: result.difficulty.od as f32,
            max_combo: result.difficulty.max_combo as i32,
        },
        PerformanceAttributes::Taiko(result) => CalculateResponse {
            stars,
            pp,
            ar: 0.0,
            od: 0.0,
            max_combo: result.difficulty.max_combo as i32,
        },
        PerformanceAttributes::Catch(result) => CalculateResponse {
            stars,
            pp,
            ar: 0.0,
            od: 0.0,
            max_combo: result.difficulty.max_combo() as i32,
        },
        PerformanceAttributes::Mania(result) => CalculateResponse {
            stars,
            pp,
            ar: 0.0,
            od: 0.0,
            max_combo: result.difficulty.max_combo as i32,
        },
    })
}

const RX: i32 = 1 << 7;

async fn calculate_play(
    Extension(ctx): Extension<Arc<Context>>,
    Json(requests): Json<Vec<CalculateRequest>>,
) -> Json<Vec<CalculateResponse>> {
    let mut results = Vec::new();

    for request in requests {
        let raw_result = if request.mods & RX > 0 && request.mode == 0 {
            calculate_relax_pp(&request, ctx.clone()).await
        } else {
            calculate_rosu_pp(&request, ctx.clone()).await
        };

        let result = match raw_result {
            Ok(result) => result,
            Err(e) => {
                log::error!(
                    "Performance calculation failed for beatmap {}: {}",
                    request.beatmap_id,
                    e.to_string()
                );

                CalculateResponse {
                    stars: 0.0,
                    pp: 0.0,
                    ar: 0.0,
                    od: 0.0,
                    max_combo: 0,
                }
            }
        };

        log::info!(
            "Calculated performance: {}pp for beatmap {}",
            result.pp,
            request.beatmap_id
        );
        results.push(result);
    }

    Json(results)
}
