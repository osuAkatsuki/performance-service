use crate::{context::Context, models::score::RippleScore, usecases};
use akatsuki_pp_rs::{Beatmap, BeatmapExt, GameMode};
use redis::AsyncCommands;
use std::{collections::HashMap, ops::DerefMut, sync::Arc, time::SystemTime};

use std::io::Write;
use tokio::sync::Mutex;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CalculateRequest {
    pub beatmap_id: i32,
    pub beatmap_md5: String,
    pub mode: i32,
    pub mods: i32,
    pub max_combo: i32,
    pub count_300: i32,
    pub count_100: i32,
    pub count_50: i32,
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

async fn calculate_special_pp(
    request: &CalculateRequest,
    context: Arc<Context>,
    recalc_ctx: &Arc<Mutex<RecalculateContext>>,
) -> anyhow::Result<CalculateResponse> {
    let mut recalc_mutex = recalc_ctx.lock().await;

    let beatmap = if recalc_mutex.beatmaps.contains_key(&request.beatmap_id) {
        recalc_mutex
            .beatmaps
            .get(&request.beatmap_id)
            .unwrap()
            .clone()
    } else {
        let beatmap_bytes =
            usecases::beatmaps::fetch_beatmap_osu_file(request.beatmap_id, context.clone()).await?;
        let beatmap = Beatmap::from_bytes(&beatmap_bytes).await?;

        recalc_mutex
            .beatmaps
            .insert(request.beatmap_id, beatmap.clone());

        beatmap
    };

    drop(recalc_mutex);

    let result = akatsuki_pp_rs::osu_2019::OsuPP::new(&beatmap)
        .mods(request.mods as u32)
        .combo(request.max_combo as usize)
        .misses(request.miss_count as usize)
        .n300(request.count_300 as usize)
        .n100(request.count_100 as usize)
        .n50(request.count_50 as usize)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    let mut stars = round(result.difficulty.stars as f32, 2);
    if stars.is_infinite() || stars.is_nan() {
        stars = 0.0;
    }

    Ok(CalculateResponse { stars, pp })
}

async fn calculate_rosu_pp(
    request: &CalculateRequest,
    context: Arc<Context>,
    recalc_ctx: &Arc<Mutex<RecalculateContext>>,
) -> anyhow::Result<CalculateResponse> {
    let mut recalc_mutex = recalc_ctx.lock().await;

    let beatmap = if recalc_mutex.beatmaps.contains_key(&request.beatmap_id) {
        recalc_mutex
            .beatmaps
            .get(&request.beatmap_id)
            .unwrap()
            .clone()
    } else {
        let beatmap_bytes =
            usecases::beatmaps::fetch_beatmap_osu_file(request.beatmap_id, context.clone()).await?;
        let beatmap = Beatmap::from_bytes(&beatmap_bytes).await?;

        recalc_mutex
            .beatmaps
            .insert(request.beatmap_id, beatmap.clone());

        beatmap
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
        .n300(request.count_300 as usize)
        .n100(request.count_100 as usize)
        .n50(request.count_50 as usize)
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

    Ok(CalculateResponse { stars, pp })
}

async fn recalculate_score(
    score: RippleScore,
    ctx: Arc<Context>,
    recalc_ctx: Arc<Mutex<RecalculateContext>>,
) -> anyhow::Result<()> {
    let request = CalculateRequest {
        beatmap_id: score.beatmap_id,
        beatmap_md5: score.beatmap_md5,
        mode: score.play_mode,
        mods: score.mods,
        max_combo: score.max_combo,
        count_300: score.count_300,
        count_100: score.count_100,
        count_50: score.count_50,
        miss_count: score.count_misses,
    };

    let response = if score.mods & RX > 0 && score.play_mode == 0 {
        calculate_special_pp(&request, ctx.clone(), &recalc_ctx).await?
    } else {
        calculate_rosu_pp(&request, ctx.clone(), &recalc_ctx).await?
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
        .execute(ctx.database.get().await?.deref_mut())
        .await?;

    log::info!(
        score_id = score.id,
        score_mode = score.play_mode,
        old_pp = score.pp,
        new_pp = score.pp;
        "Recalculated score",
    );

    Ok(())
}

async fn recalculate_mode_scores(
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    recalc_ctx: Arc<Mutex<RecalculateContext>>,
    mods_value: Option<i32>,
) -> anyhow::Result<()> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let mods_query_str = match mods_value {
        Some(mods) => format!("AND (mods & {}) > 0", mods),
        None => "".to_string(),
    };

    let scores: Vec<RippleScore> = sqlx::query_as(
        &format!(
            "SELECT s.id, s.beatmap_md5, s.userid, s.score, s.max_combo, s.full_combo, s.mods, s.300_count,
            s.100_count, s.50_count, s.katus_count, s.gekis_count, s.misses_count, s.time, s.play_mode, s.completed,
            s.accuracy, s.pp, s.checksum, s.patcher, s.pinned, b.beatmap_id, b.beatmapset_id, b.song_name
            FROM {} s
            INNER JOIN
                beatmaps b
                USING(beatmap_md5)
            WHERE
                completed IN (2, 3)
                AND play_mode = ?
                {}
            ORDER BY pp DESC",
            scores_table,
            mods_query_str,
        )
    )
    .bind(mode)
    .fetch_all(ctx.database.get().await?.deref_mut())
    .await?;

    for score_chunk in scores.chunks(100).map(|c| c.to_vec()) {
        let mut futures = Vec::new();

        for score in score_chunk {
            let future = tokio::spawn(recalculate_score(score, ctx.clone(), recalc_ctx.clone()));
            futures.push(future);
        }

        futures::future::try_join_all(futures).await?;
    }

    Ok(())
}

fn calculate_new_pp(scores: &Vec<RippleScore>, score_count: i32) -> i32 {
    let mut total_pp = 0.0;

    for (idx, score) in scores.iter().enumerate() {
        total_pp += score.pp * 0.95_f32.powi(idx as i32);
    }

    // bonus pp
    total_pp += 416.6667 * (1.0 - 0.995_f32.powi(score_count as i32));

    total_pp.round() as i32
}

async fn recalculate_status(
    user_id: i32,
    mode: i32,
    rx: i32,
    beatmap_md5: String,
    ctx: Arc<Context>,
) -> anyhow::Result<()> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let scores: Vec<(i64, f32)> = sqlx::query_as(
        &format!(
            "SELECT id, pp FROM {} WHERE userid = ? AND play_mode = ? AND beatmap_md5 = ? AND completed IN (2, 3) ORDER BY pp DESC",
            scores_table
        )
    )
    .bind(user_id)
    .bind(mode)
    .bind(beatmap_md5)
    .fetch_all(ctx.database.get().await?.deref_mut())
    .await?;

    let best_id = scores[0].0;
    let non_bests = scores[1..].to_vec();

    sqlx::query(&format!(
        "UPDATE {} SET completed = 3 WHERE id = ?",
        scores_table
    ))
    .bind(best_id)
    .execute(ctx.database.get().await?.deref_mut())
    .await?;

    for non_best in non_bests {
        sqlx::query(&format!(
            "UPDATE {} SET completed = 2 WHERE id = ?",
            scores_table
        ))
        .bind(non_best.0)
        .execute(ctx.database.get().await?.deref_mut())
        .await?;
    }

    Ok(())
}

async fn recalculate_statuses(
    user_id: i32,
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
) -> anyhow::Result<()> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let beatmap_md5s: Vec<(String,)> = sqlx::query_as(
        &format!(
            "SELECT DISTINCT (beatmap_md5) FROM {} WHERE userid = ? AND completed IN (2, 3) AND play_mode = ?",
            scores_table
        )
    )
        .bind(user_id)
        .bind(mode)
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?;

    for beatmap_chunk in beatmap_md5s.chunks(100).map(|c| c.to_vec()) {
        let mut futures = Vec::with_capacity(beatmap_chunk.len());

        for (beatmap_md5,) in beatmap_chunk {
            let future = tokio::spawn(recalculate_status(
                user_id,
                mode,
                rx,
                beatmap_md5,
                ctx.clone(),
            ));

            futures.push(future);
        }

        futures::future::try_join_all(futures).await?;
    }

    Ok(())
}

async fn recalculate_user(
    user_id: i32,
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
) -> anyhow::Result<()> {
    recalculate_statuses(user_id, mode, rx, ctx.clone()).await?;

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
            s.accuracy, s.pp, s.checksum, s.patcher, s.pinned, b.beatmap_id, b.beatmapset_id, b.song_name
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
    .fetch_all(ctx.database.get().await?.deref_mut())
    .await?;

    let score_count: i32 = sqlx::query_scalar(
        &format!(
            "SELECT COUNT(s.id) FROM {} s INNER JOIN beatmaps USING(beatmap_md5) WHERE userid = ? AND completed = 3 AND play_mode = ? AND ranked IN (3, 2) LIMIT 1000",
            scores_table
        )
    )
        .bind(user_id)
        .bind(mode)
        .fetch_one(ctx.database.get().await?.deref_mut())
        .await?;

    let new_pp = calculate_new_pp(&scores, score_count);

    sqlx::query(&format!(
        "UPDATE user_stats SET pp = ? WHERE user_id = ? AND mode = ?",
    ))
    .bind(new_pp)
    .bind(user_id)
    .bind(mode + (4 * rx))
    .execute(ctx.database.get().await?.deref_mut())
    .await?;

    let (country, user_privileges): (String, i32) =
        sqlx::query_as("SELECT country, privileges FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(ctx.database.get().await?.deref_mut())
            .await?;

    let last_score_time: Option<i32> = sqlx::query_scalar(&format!(
        "SELECT max(time) FROM {} INNER JOIN beatmaps USING(beatmap_md5)
            WHERE userid = ? AND completed = 3 AND ranked IN (2, 3) AND play_mode = ?
            ORDER BY pp DESC LIMIT 100",
        scores_table
    ))
    .bind(user_id)
    .bind(mode)
    .fetch_optional(ctx.database.get().await?.deref_mut())
    .await?;

    let inactive_days = match last_score_time {
        Some(time) => {
            ((SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
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

        let stats_prefix = match mode {
            0 => "std",
            1 => "taiko",
            2 => "ctb",
            3 => "mania",
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
        user_id = user_id,
        mode = mode,
        relax = rx,
        new_pp = new_pp;
        "Recalculated user",
    );

    Ok(())
}

async fn recalculate_mode_users(mode: i32, rx: i32, ctx: Arc<Context>) -> anyhow::Result<()> {
    let user_ids: Vec<(i32,)> = sqlx::query_as(&format!("SELECT id FROM users"))
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?;

    for user_id_chunk in user_ids.chunks(100).map(|c| c.to_vec()) {
        let mut futures = Vec::with_capacity(user_id_chunk.len());

        for (user_id,) in user_id_chunk {
            let future = tokio::spawn(recalculate_user(user_id, mode, rx, ctx.clone()));
            futures.push(future);
        }

        futures::future::try_join_all(futures).await?;
    }

    Ok(())
}

struct RecalculateContext {
    pub beatmaps: HashMap<i32, Beatmap>,
}

struct DeployArgs {
    modes: Vec<i32>,
    relax_bits: Vec<i32>,
    total_pp_only: bool,
    mods_filter: Option<i32>,
}

fn deploy_args_from_env() -> anyhow::Result<DeployArgs> {
    let modes_str = std::env::var("DEPLOY_MODES")?;
    let relax_bits_str = std::env::var("DEPLOY_RELAX_BITS")?;
    let total_pp_only_str = std::env::var("DEPLOY_TOTAL_PP_ONLY").unwrap_or("".to_string());
    let mods_filter_str = std::env::var("DEPLOY_MODS_FILTER").ok();

    Ok(DeployArgs {
        modes: modes_str
            .trim()
            .split(',')
            .map(|s| s.parse::<i32>().expect("failed to parse mode"))
            .collect::<Vec<_>>(),
        relax_bits: relax_bits_str
            .trim()
            .split(',')
            .map(|s| s.parse::<i32>().expect("failed to parse relax bits"))
            .collect::<Vec<_>>(),
        total_pp_only: total_pp_only_str.to_lowercase().trim() == "1",
        mods_filter: mods_filter_str
            .map(|mods| mods.trim().parse::<i32>().expect("failed to parse mods")),
    })
}

fn deploy_args_from_input() -> anyhow::Result<DeployArgs> {
    print!("Enter the modes (comma delimited) to deploy: ");
    std::io::stdout().flush()?;

    let mut modes_str = String::new();
    std::io::stdin().read_line(&mut modes_str)?;
    let modes = modes_str
        .trim()
        .split(',')
        .map(|s| s.parse::<i32>().expect("failed to parse mode"))
        .collect::<Vec<_>>();

    print!("\n");
    std::io::stdout().flush()?;

    print!("Enter the relax bits (comma delimited) to deploy: ");
    std::io::stdout().flush()?;

    let mut relax_str = String::new();
    std::io::stdin().read_line(&mut relax_str)?;
    let relax_bits = relax_str
        .trim()
        .split(',')
        .map(|s| s.parse::<i32>().expect("failed to parse relax bits"))
        .collect::<Vec<_>>();

    print!("\n");
    std::io::stdout().flush()?;

    print!("Total PP recalc only (y/n): ");
    std::io::stdout().flush()?;

    let mut total_only_str = String::new();
    std::io::stdin().read_line(&mut total_only_str)?;
    let total_only = total_only_str.to_lowercase().trim() == "y";

    print!("\n");
    std::io::stdout().flush()?;

    print!("Mod value recalc only (y/n): ");
    std::io::stdout().flush()?;

    let mut mod_recalc_value_only_str = String::new();
    std::io::stdin().read_line(&mut mod_recalc_value_only_str)?;
    let mod_recalc_value_only = mod_recalc_value_only_str.to_lowercase().trim() == "y";

    print!("\n");
    std::io::stdout().flush()?;

    let mut mods_value: Option<i32> = None;
    if mod_recalc_value_only {
        print!("Mods value (int): ");
        std::io::stdout().flush()?;

        let mut mods_value_str = String::new();
        std::io::stdin().read_line(&mut mods_value_str)?;
        mods_value = Some(
            mods_value_str
                .trim()
                .parse::<i32>()
                .expect("failed to parse mods"),
        );

        print!("\n");
        std::io::stdout().flush()?;
    }

    Ok(DeployArgs {
        modes,
        relax_bits,
        total_pp_only: total_only,
        mods_filter: mods_value,
    })
}

fn retrieve_deploy_args() -> anyhow::Result<DeployArgs> {
    let env_deploy_args = deploy_args_from_env();

    if let Ok(deploy_args) = env_deploy_args {
        Ok(deploy_args)
    } else {
        deploy_args_from_input()
    }
}

pub async fn serve(context: Context) -> anyhow::Result<()> {
    let deploy_args = retrieve_deploy_args()?;

    let recalculate_context = Arc::new(Mutex::new(RecalculateContext {
        beatmaps: HashMap::new(),
    }));

    let context_arc = Arc::new(context);

    if !deploy_args.total_pp_only {
        for mode in &deploy_args.modes {
            let mode = mode.clone();

            let rx = vec![0, 1, 2].contains(&mode);
            let ap = mode == 0;

            if rx || ap {
                for rx in &deploy_args.relax_bits {
                    recalculate_mode_scores(
                        mode,
                        rx.clone(),
                        context_arc.clone(),
                        recalculate_context.clone(),
                        deploy_args.mods_filter,
                    )
                    .await?;
                }
            } else {
                recalculate_mode_scores(
                    mode,
                    0,
                    context_arc.clone(),
                    recalculate_context.clone(),
                    deploy_args.mods_filter,
                )
                .await?;
            }
        }
    }

    for mode in &deploy_args.modes {
        let mode = mode.clone();

        let rx = vec![0, 1, 2].contains(&mode);
        let ap = mode == 0;

        if rx || ap {
            for rx in &deploy_args.relax_bits {
                recalculate_mode_users(mode, rx.clone(), context_arc.clone()).await?;
            }
        } else {
            recalculate_mode_users(mode, 0, context_arc.clone()).await?;
        }
    }

    Ok(())
}
