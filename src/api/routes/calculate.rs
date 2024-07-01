use crate::api::error::ApiError;
use crate::usecases;
use crate::{
    api::error::AppResult,
    context::Context,
    errors::{Error, ErrorCode},
};
use akatsuki_pp_rs::{Beatmap, BeatmapExt, GameMode, PerformanceAttributes};
use axum::response::IntoResponse;
use axum::{extract::Extension, routing::post, Json, Router};
use reqwest::StatusCode;
use std::sync::Arc;

pub fn router() -> Router {
    Router::new().route("/api/v1/calculate", post(calculate_play))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CalculateRequest {
    pub beatmap_id: i32,
    pub beatmap_md5: String,
    pub mode: i32,
    pub mods: i32,
    pub max_combo: i32,
    pub accuracy: Option<f32>,
    pub count_300: Option<i32>,
    pub count_100: Option<i32>,
    pub count_50: Option<i32>,
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
) -> Result<CalculateResponse, Error> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(request.beatmap_id, context).await?;
    let beatmap = Beatmap::from_bytes(&beatmap_bytes)
        .await
        .map_err(|_| Error {
            error_code: ErrorCode::InternalServerError,
            user_feedback: "Failed to parse beatmap",
        })?;

    let mut calculate = akatsuki_pp_rs::osu_2019::OsuPP::new(&beatmap)
        .mods(request.mods as u32)
        .combo(request.max_combo as usize);

    calculate = calculate.misses(request.miss_count as usize);
    if request.accuracy.is_some() {
        calculate = calculate.accuracy(request.accuracy.unwrap());
    } else {
        calculate = calculate
            .n300(request.count_300.unwrap() as usize)
            .n100(request.count_100.unwrap() as usize)
            .n50(request.count_50.unwrap() as usize);
    }

    let result = calculate.calculate();

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
) -> Result<CalculateResponse, Error> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(request.beatmap_id, context).await?;

    let beatmap = Beatmap::from_bytes(&beatmap_bytes)
        .await
        .map_err(|_| Error {
            error_code: ErrorCode::InternalServerError,
            user_feedback: "Failed to parse beatmap",
        })?;

    let mut calculate = beatmap
        .pp()
        .mode(match request.mode {
            0 => GameMode::Osu,
            1 => GameMode::Taiko,
            2 => GameMode::Catch,
            3 => GameMode::Mania,
            _ => {
                return Err(Error {
                    error_code: ErrorCode::BadRequest,
                    user_feedback: "Invalid mode",
                })
            }
        })
        .mods(request.mods as u32)
        .combo(request.max_combo as usize);

    calculate = calculate.n_misses(request.miss_count as usize);
    if request.accuracy.is_some() {
        calculate = calculate.accuracy(request.accuracy.unwrap() as f64);
    } else {
        calculate = calculate
            .n300(request.count_300.unwrap() as usize)
            .n100(request.count_100.unwrap() as usize)
            .n50(request.count_50.unwrap() as usize);
    }

    let result = calculate.calculate();

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
) -> AppResult<impl IntoResponse> {
    let mut results = Vec::new();

    for request in requests {
        let have_hit_statistics = request.count_300.is_some()
            && request.count_100.is_some()
            && request.count_50.is_some();
        let have_accuracy = request.accuracy.is_some();

        if (!have_accuracy && !have_hit_statistics) || (have_accuracy && have_hit_statistics) {
            return Ok((
                StatusCode::BAD_REQUEST,
                "you must pass accuracy OR hit results",
            )
                .into_response());
        }

        let raw_result = if request.mods & RX > 0 && request.mode == 0 {
            calculate_relax_pp(&request, ctx.clone()).await
        } else {
            calculate_rosu_pp(&request, ctx.clone()).await
        };

        let result = match raw_result {
            Ok(result) => result,
            Err(e) => {
                log::error!(
                    beatmap_id = request.beatmap_id,
                    error = e.user_feedback;
                    "Performance calculation failed for beatmap",
                );

                return Err(ApiError(e));
            }
        };

        log::info!(
            performance_points = result.pp,
            star_rating = result.stars,
            beatmap_id = request.beatmap_id;
            "Calculated performance for beatmap.",
        );
        results.push(result);
    }

    Ok(Json(results).into_response())
}
