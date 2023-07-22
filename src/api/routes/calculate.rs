use crate::context::Context;
use akatsuki_pp_rs::{Beatmap, BeatmapExt, GameMode, PerformanceAttributes};
use axum::{extract::Extension, routing::post, Json, Router};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File;

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
    beatmap_path: PathBuf,
    request: &CalculateRequest,
) -> CalculateResponse {
    let beatmap = match Beatmap::from_path(beatmap_path).await {
        Ok(beatmap) => beatmap,
        Err(_) => {
            return CalculateResponse {
                stars: 0.0,
                pp: 0.0,
                ar: 0.0,
                od: 0.0,
                max_combo: 0,
            }
        }
    };

    let result = akatsuki_pp_rs::osu_2019::OsuPP::new(&beatmap)
        .mods(request.mods as u32)
        .combo(request.max_combo as usize)
        .misses(request.miss_count as usize)
        .accuracy(request.accuracy)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    let mut stars = round(result.difficulty.stars as f32, 2);
    if stars.is_infinite() || stars.is_nan() {
        stars = 0.0;
    }

    CalculateResponse {
        stars,
        pp,
        ar: result.difficulty.ar as f32,
        od: result.difficulty.od as f32,
        max_combo: result.difficulty.max_combo as i32,
    }
}

async fn calculate_rosu_pp(beatmap_path: PathBuf, request: &CalculateRequest) -> CalculateResponse {
    let beatmap = match Beatmap::from_path(beatmap_path).await {
        Ok(beatmap) => beatmap,
        Err(_) => {
            return CalculateResponse {
                stars: 0.0,
                pp: 0.0,
                ar: 0.0,
                od: 0.0,
                max_combo: 0,
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

    match result {
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
    }
}

const RX: i32 = 1 << 7;

async fn download_beatmap(beatmap_path: PathBuf, request: &CalculateRequest) -> anyhow::Result<()> {
    let response = reqwest::get(&format!("https://old.ppy.sh/osu/{}", request.beatmap_id))
        .await?
        .error_for_status()?;

    let mut file = File::create(&beatmap_path).await?;
    let mut content = Cursor::new(response.bytes().await?);
    tokio::io::copy(&mut content, &mut file).await?;

    Ok(())
}

async fn calculate_play(
    Extension(ctx): Extension<Arc<Context>>,
    Json(requests): Json<Vec<CalculateRequest>>,
) -> Json<Vec<CalculateResponse>> {
    let mut results = Vec::new();

    for request in requests {
        let beatmap_path =
            Path::new(&ctx.config.beatmaps_path).join(format!("{}.osu", request.beatmap_id));

        if !beatmap_path.exists() {
            match download_beatmap(beatmap_path.clone(), &request).await {
                Ok(_) => {}
                Err(_) => {
                    results.push(CalculateResponse {
                        stars: 0.0,
                        pp: 0.0,
                        ar: 0.0,
                        od: 0.0,
                        max_combo: 0,
                    });

                    continue;
                }
            }
        }

        let result = if request.mods & RX > 0 && request.mode == 0 {
            calculate_relax_pp(beatmap_path, &request).await
        } else {
            calculate_rosu_pp(beatmap_path, &request).await
        };

        results.push(result);
    }

    Json(results)
}
