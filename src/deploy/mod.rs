use crate::{context::Context, models::score::RippleScore};
use akatsuki_pp_rs::{Beatmap, BeatmapExt, GameMode};
use oppai_rs::{Combo, Mods as OppaiMods, Oppai};
use redis::AsyncCommands;
use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use tokio::fs::File;
use tokio::sync::Mutex;

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

const RX: i32 = 1 << 7;
const AP: i32 = 1 << 13;

async fn calculate_oppai_pp(
    beatmap_path: PathBuf,
    request: &CalculateRequest,
) -> anyhow::Result<CalculateResponse> {
    let oppai: &mut Oppai = &mut Oppai::new(&beatmap_path)?;

    let final_oppai = match oppai
        .mods(OppaiMods::from_bits_truncate(request.mods))
        .combo(Combo::NonFC {
            max_combo: request.max_combo as u32,
            misses: request.miss_count as u32,
        }) {
        Ok(oppai) => oppai,
        Err(_) => oppai.combo(Combo::FC(0))?,
    }
    .accuracy(request.accuracy)?;

    let (mut pp, mut stars) = final_oppai.run();
    pp = round(pp, 2);
    stars = round(stars, 2);

    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    if stars.is_infinite() || stars.is_nan() {
        stars = 0.0;
    }

    Ok(CalculateResponse { stars, pp })
}

async fn calculate_bancho_pp(
    beatmap_path: PathBuf,
    request: &CalculateRequest,
    recalc_ctx: &Arc<Mutex<RecalculateContext>>,
) -> CalculateResponse {
    let mut recalc_mutex = recalc_ctx.lock().await;

    let beatmap = if recalc_mutex.beatmaps.contains_key(&request.beatmap_id) {
        recalc_mutex
            .beatmaps
            .get(&request.beatmap_id)
            .unwrap()
            .clone()
    } else {
        match Beatmap::from_path(beatmap_path).await {
            Ok(beatmap) => {
                recalc_mutex
                    .beatmaps
                    .insert(request.beatmap_id, beatmap.clone());

                beatmap
            }
            Err(_) => {
                return CalculateResponse {
                    stars: 0.0,
                    pp: 0.0,
                }
            }
        }
    };

    drop(recalc_mutex);

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

async fn recalculate_score(
    score: RippleScore,
    beatmap_path: PathBuf,
    ctx: &Arc<Context>,
    recalc_ctx: &Arc<Mutex<RecalculateContext>>,
) -> anyhow::Result<()> {
    let request = CalculateRequest {
        beatmap_id: score.beatmap_id,
        mode: score.play_mode,
        mods: score.mods,
        max_combo: score.max_combo,
        accuracy: score.accuracy,
        miss_count: score.count_misses,
    };

    let response =
        if (score.mods & RX > 0 || score.mods & AP > 0) && vec![0, 1].contains(&score.play_mode) {
            match calculate_oppai_pp(beatmap_path, &request).await {
                Ok(response) => response,
                Err(e) => {
                    log::warn!("{}", e);
                    CalculateResponse {
                        stars: 0.0,
                        pp: 0.0,
                    }
                }
            }
        } else {
            calculate_bancho_pp(beatmap_path, &request, recalc_ctx).await
        };

    let rx = if score.mods & RX > 0 {
        1
    } else if score.mods & AP > 0 {
        2
    } else {
        0
    };

    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    sqlx::query(&format!("UPDATE {} SET pp = ? WHERE id = ?", scores_table))
        .bind(response.pp)
        .bind(score.id)
        .execute(&ctx.database)
        .await?;

    // cache will only contain it if it's their best score
    if score.completed == 3 {
        let mut redis_connection = ctx.redis.get_async_connection().await?;
        redis_connection
            .publish(
                "cache:update_score_pp",
                serde_json::json!({
                    "beatmap_id": score.beatmap_id,
                    "user_id": score.userid,
                    "score_id": score.id,
                    "new_pp": response.pp,
                    "mode_vn": score.play_mode,
                    "relax": rx,
                })
                .to_string(),
            )
            .await?;
    }

    log::info!(
        "Recalculated score ID {} (mode: {}) | {} -> {}",
        score.id,
        score.play_mode,
        score.pp,
        response.pp,
    );

    Ok(())
}

async fn recalculate_mode_scores(
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    recalc_ctx: Arc<Mutex<RecalculateContext>>,
) -> anyhow::Result<()> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let scores: Vec<RippleScore> = sqlx::query_as(
        &format!(
            "SELECT s.id, s.beatmap_md5, s.userid, s.score, s.max_combo, s.full_combo, s.mods, s.300_count, 
            s.100_count, s.50_count, s.katus_count, s.gekis_count, s.misses_count, s.time, s.play_mode, s.completed, 
            s.accuracy, s.pp, s.checksum, s.patcher, s.pinned, b.beatmap_id, b.beatmapset_id 
            FROM {} s 
            INNER JOIN 
                beatmaps b 
                USING(beatmap_md5) 
            WHERE 
                completed IN (2, 3) 
                AND play_mode = ? 
            ORDER BY pp DESC",
            scores_table
        )
    )
    .bind(mode)
    .fetch_all(&ctx.database)
    .await?;

    for score in scores {
        let beatmap_path =
            Path::new(&ctx.config.beatmaps_path).join(format!("{}.osu", score.beatmap_id));

        if !beatmap_path.exists() {
            let response = reqwest::get(&format!("https://old.ppy.sh/osu/{}", score.beatmap_id))
                .await?
                .error_for_status()?;

            let mut file = File::create(&beatmap_path).await?;
            let mut content = Cursor::new(response.bytes().await?);
            tokio::io::copy(&mut content, &mut file).await?;
        }

        recalculate_score(score, beatmap_path, &ctx, &recalc_ctx).await?;
    }

    Ok(())
}

fn calculate_new_pp(scores: &Vec<RippleScore>, score_count: i32) -> i32 {
    let mut total_pp = 0.0;

    for (idx, score) in scores.iter().enumerate() {
        total_pp += score.pp * 0.95_f32.powi(idx as i32);
    }

    // bonus pp
    total_pp += 416.6667 * (1.0 - 0.9994_f32.powi(score_count as i32));

    total_pp.round() as i32
}

async fn recalculate_user(
    user_id: i32,
    mode: i32,
    rx: i32,
    ctx: &Arc<Context>,
) -> anyhow::Result<()> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let scores: Vec<RippleScore> = sqlx::query_as(
        &format!(
            "SELECT s.id, s.beatmap_md5, s.userid, s.score, s.max_combo, s.full_combo, s.mods, s.300_count, 
            s.100_count, s.50_count, s.katus_count, s.gekis_count, s.misses_count, s.time, s.play_mode, s.completed, 
            s.accuracy, s.pp, s.checksum, s.patcher, s.pinned, b.beatmap_id, b.beatmapset_id 
            FROM {} s 
            INNER JOIN 
                beatmaps b 
                USING(beatmap_md5) 
            WHERE 
                userid = ? 
                AND completed = 3 
                AND play_mode = ? 
                AND ranked IN (3, 2) 
            ORDER BY pp DESC 
            LIMIT 100",
            scores_table
        )
    )
    .bind(user_id)
    .bind(mode)
    .fetch_all(&ctx.database)
    .await?;

    let score_count: i32 = sqlx::query_scalar(
        &format!(
            "SELECT COUNT(s.id) FROM {} s INNER JOIN beatmaps USING(beatmap_md5) WHERE userid = ? AND completed = 3 AND play_mode = ? AND ranked IN (3, 2) LIMIT 25397",
            scores_table
        )
    )
        .bind(user_id)
        .bind(mode)
        .fetch_one(&ctx.database)
        .await?;

    let new_pp = calculate_new_pp(&scores, score_count);

    let stats_table = match rx {
        0 => "users_stats",
        1 => "rx_stats",
        2 => "ap_stats",
        _ => unreachable!(),
    };

    let stats_prefix = match mode {
        0 => "std",
        1 => "taiko",
        2 => "ctb",
        3 => "mania",
        _ => unreachable!(),
    };

    sqlx::query(&format!(
        "UPDATE {} SET pp_{} = ? WHERE id = ?",
        stats_table, stats_prefix
    ))
    .bind(new_pp)
    .bind(user_id)
    .execute(&ctx.database)
    .await?;

    let (country, user_privileges): (String, i32) = sqlx::query_as(
        "SELECT country, privileges FROM users INNER JOIN users_stats USING(id) WHERE id = ?",
    )
    .bind(user_id)
    .fetch_one(&ctx.database)
    .await?;

    let last_score_time: Option<i32> = sqlx::query_scalar(&format!(
        "SELECT max(time) FROM {} INNER JOIN beatmaps USING(beatmap_md5) 
            WHERE userid = ? AND completed = 3 AND ranked IN (2, 3) AND play_mode = ? 
            ORDER BY pp DESC LIMIT 100",
        scores_table
    ))
    .bind(user_id)
    .bind(mode)
    .fetch_optional(&ctx.database)
    .await
    .unwrap_or(None);

    let inactive_days = match last_score_time {
        Some(time) => {
            ((SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i32)
                - time)
                / 60
                / 60
                / 24
        }
        None => 60,
    };

    let mut redis_connection = ctx.redis.get_async_connection().await?;

    // unrestricted, and set a score in the past 2 months
    if user_privileges & 1 > 0 && inactive_days < 60 {
        let redis_leaderboard = match rx {
            0 => "leaderboard".to_string(),
            1 => "relaxboard".to_string(),
            2 => "autoboard".to_string(),
            _ => unreachable!(),
        };

        redis_connection
            .zadd(
                format!("ripple:{}:{}", redis_leaderboard, stats_prefix),
                user_id.to_string(),
                new_pp,
            )
            .await?;

        redis_connection
            .zadd(
                format!(
                    "ripple:{}:{}:{}",
                    redis_leaderboard,
                    stats_prefix,
                    country.to_lowercase()
                ),
                user_id.to_string(),
                new_pp,
            )
            .await?;
    }

    redis_connection
        .publish("peppy:update_cached_stats", user_id)
        .await?;

    log::info!(
        "Recalculated user {} in mode {} (rx: {}) | pp: {}",
        user_id,
        mode,
        rx,
        new_pp
    );

    Ok(())
}

async fn recalculate_mode_users(mode: i32, rx: i32, ctx: Arc<Context>) -> anyhow::Result<()> {
    let user_ids: Vec<(i32,)> = sqlx::query_as(&format!("SELECT id FROM users"))
        .fetch_all(&ctx.database)
        .await?;

    for (user_id,) in user_ids {
        recalculate_user(user_id, mode, rx, &ctx).await?;
    }

    Ok(())
}

struct RecalculateContext {
    pub beatmaps: HashMap<i32, Beatmap>,
}

pub async fn serve(context: Context) -> anyhow::Result<()> {
    let recalculate_context = Arc::new(Mutex::new(RecalculateContext {
        beatmaps: HashMap::new(),
    }));

    let context_arc = Arc::new(context);

    for mode in vec![1, 2, 3] {
        //for mode in vec![0, 1, 2, 3] {
        let rx = vec![0, 1, 2].contains(&mode);
        let ap = mode == 0;

        if rx || ap {
            for rx in vec![0, 1, 2] {
                recalculate_mode_scores(mode, rx, context_arc.clone(), recalculate_context.clone())
                    .await?;
            }
        } else {
            recalculate_mode_scores(mode, 0, context_arc.clone(), recalculate_context.clone())
                .await?;
        }
    }

    for mode in vec![0, 1, 2, 3] {
        let rx = vec![0, 1, 2].contains(&mode);
        let ap = mode == 0;

        if rx || ap {
            for rx in vec![0, 1, 2] {
                recalculate_mode_users(mode, rx, context_arc.clone()).await?;
            }
        } else {
            recalculate_mode_users(mode, 0, context_arc.clone()).await?;
        }
    }

    Ok(())
}
