use crate::{context::Context, usecases};
use akatsuki_pp_rs::model::mode::GameMode;
use akatsuki_pp_rs::Beatmap;
use anyhow::anyhow;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use redis::AsyncCommands;
use std::collections::HashMap;
use std::io::Write;
use std::{ops::DerefMut, sync::Arc, time::SystemTime};
use tokio::sync::Semaphore;

#[derive(Clone, sqlx::FromRow)]
struct LightweightScore {
    pub id: i64,
    pub mods: i32,
    pub max_combo: i32,
    pub play_mode: i32,
    pub beatmap_id: i32,
    pub pp: f32,
    pub accuracy: f32,

    #[sqlx(rename = "misses_count")]
    pub count_misses: i32,
}

fn round(x: f32, decimals: u32) -> f32 {
    let y = 10i32.pow(decimals) as f32;
    (x * y).round() / y
}

const MAX_CONCURRENT_BEATMAP_TASKS: usize = 10;
const MAX_CONCURRENT_TASKS: usize = 100;
const BATCH_SIZE: u32 = 1000;

fn group_scores_by_mods(vec: Vec<LightweightScore>) -> HashMap<i32, Vec<LightweightScore>> {
    let mut grouped_map = HashMap::new();

    for item in vec {
        grouped_map
            .entry(item.mods)
            .or_insert_with(Vec::new)
            .push(item);
    }

    grouped_map
}

async fn recalculate_relax_scores(
    scores: Vec<LightweightScore>,
    mods: i32,
    scores_table: &str,
    beatmap: &Beatmap,
    ctx: Arc<Context>,
) -> anyhow::Result<()> {
    let difficulty_attributes =
        akatsuki_pp_rs::osu_2019::stars::stars(&beatmap, (mods as u32).into(), None);

    for score in scores {
        let result =
            akatsuki_pp_rs::osu_2019::OsuPP::from_attributes(difficulty_attributes.clone())
                .mods(score.mods as u32)
                .combo(score.max_combo as u32)
                .misses(score.count_misses as u32)
                .accuracy(score.accuracy)
                .calculate();

        let mut pp = round(result.pp as f32, 2);
        if pp.is_infinite() || pp.is_nan() {
            pp = 0.0;
        }

        sqlx::query(&format!("UPDATE {} SET pp = ? WHERE id = ?", scores_table))
            .bind(pp)
            .bind(score.id)
            .execute(ctx.database.get().await?.deref_mut())
            .await?;
    }

    Ok(())
}

async fn recalculate_scores(
    mut scores: Vec<LightweightScore>,
    scores_table: &str,
    beatmap: &Beatmap,
    ctx: Arc<Context>,
) -> anyhow::Result<()> {
    let first_score = scores[0].clone();

    let result = beatmap
        .performance()
        .try_mode(match first_score.play_mode {
            0 => GameMode::Osu,
            1 => GameMode::Taiko,
            2 => GameMode::Catch,
            3 => GameMode::Mania,
            _ => unreachable!(),
        })
        .map_err(|_| {
            anyhow!(
                "failed to set mode {} for beatmap {}",
                first_score.play_mode,
                first_score.beatmap_id
            )
        })?
        .mods(first_score.mods as u32)
        .lazer(false)
        .combo(first_score.max_combo as u32)
        .misses(first_score.count_misses as u32)
        .accuracy(first_score.accuracy as f64)
        .calculate();

    sqlx::query(&format!("UPDATE {} SET pp = ? WHERE id = ?", scores_table))
        .bind(result.pp())
        .bind(first_score.id)
        .execute(ctx.database.get().await?.deref_mut())
        .await?;

    let difficulty_attributes = result.difficulty_attributes();

    for score in &mut scores[1..] {
        let result = difficulty_attributes
            .clone()
            .performance()
            .try_mode(match score.play_mode {
                0 => GameMode::Osu,
                1 => GameMode::Taiko,
                2 => GameMode::Catch,
                3 => GameMode::Mania,
                _ => unreachable!(),
            })
            .map_err(|_| {
                anyhow!(
                    "failed to set mode {} for beatmap {}",
                    score.play_mode,
                    score.beatmap_id
                )
            })?
            .mods(score.mods as u32)
            .lazer(false)
            .combo(score.max_combo as u32)
            .misses(score.count_misses as u32)
            .accuracy(score.accuracy as f64)
            .calculate();

        let mut pp = round(result.pp() as f32, 2);
        if pp.is_infinite() || pp.is_nan() {
            pp = 0.0;
        }

        sqlx::query(&format!("UPDATE {} SET pp = ? WHERE id = ?", scores_table))
            .bind(pp)
            .bind(score.id)
            .execute(ctx.database.get().await?.deref_mut())
            .await?;
    }

    Ok(())
}

async fn recalculate_beatmap(
    beatmap_md5: String,
    scores_table: &str,
    mods_query_str: String,
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
) -> anyhow::Result<()> {
    let scores: Vec<LightweightScore> = sqlx::query_as(&format!(
        "SELECT s.id, s.mods, s.max_combo, s.play_mode, b.beatmap_id, s.pp, s.accuracy, s.misses_count
        FROM {} s
        INNER JOIN
            beatmaps b
            USING(beatmap_md5)
        WHERE
            completed IN (2, 3)
            AND play_mode = ?
            AND s.beatmap_md5 = ?
            {}
        ORDER BY pp DESC",
        scores_table, mods_query_str,
    ))
    .bind(mode)
    .bind(beatmap_md5)
    .fetch_all(ctx.database.get().await?.deref_mut())
    .await?;

    if scores.is_empty() {
        return Ok(());
    }

    let base_score = scores[0].clone();
    let score_count = scores.len();

    let grouped_scores = group_scores_by_mods(scores);

    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(base_score.beatmap_id, ctx.clone()).await?;

    let beatmap = Beatmap::from_bytes(&beatmap_bytes)?;

    for (mods, mod_scores) in grouped_scores {
        if mode == 0 && rx == 1 {
            recalculate_relax_scores(mod_scores, mods, scores_table, &beatmap, ctx.clone()).await?;
        } else {
            recalculate_scores(mod_scores, scores_table, &beatmap, ctx.clone()).await?;
        }
    }

    log::info!(
        beatmap_id = base_score.beatmap_id,
        score_count = score_count,
        mode = mode,
        rx = rx;
        "Recalculated beatmap"
    );

    Ok(())
}

async fn recalculate_mode_scores(
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    mods_value: Option<i32>,
    neq_mods_value: Option<i32>,
    mapper_filter: Option<String>,
    map_filter: Option<Vec<i32>>,
) -> anyhow::Result<()> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let mods_query_str = if let Some(mods_value) = mods_value {
        format!("AND (mods & {}) > 0", mods_value)
    } else if let Some(neq_mods_value) = neq_mods_value {
        format!("AND (mods & {}) = 0", neq_mods_value)
    } else {
        "".to_string()
    };

    let beatmap_md5s: Vec<(String,)> = if let Some(mapper_filter) = mapper_filter {
        sqlx::query_as(&format!(
            "SELECT beatmap_md5, COUNT(*) AS c FROM {} INNER JOIN beatmaps USING(beatmap_md5) 
            WHERE completed IN (2, 3) AND play_mode = ? {} AND beatmaps.file_name LIKE ? GROUP BY beatmap_md5 ORDER BY c DESC",
            scores_table, mods_query_str,
        ))
        .bind(mode)
        .bind(format!("%({mapper_filter})%"))
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?
    } else if let Some(map_filter) = map_filter {
        let formatted_beatmap_ids = format!(
            "({})",
            map_filter
                .iter()
                .map(|map| map.to_string())
                .collect::<Vec<String>>()
                .join(",")
        );
        sqlx::query_as(&format!(
            "SELECT beatmap_md5, COUNT(*) AS c FROM {} INNER JOIN beatmaps USING(beatmap_md5) 
            WHERE completed IN (2, 3) AND play_mode = ? {} AND beatmaps.beatmap_id IN {} GROUP BY beatmap_md5 ORDER BY c DESC",
            scores_table, mods_query_str, formatted_beatmap_ids
        ))
        .bind(mode)
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT beatmap_md5, COUNT(*) AS c FROM {} WHERE completed IN (2, 3) AND play_mode = ? {} GROUP BY beatmap_md5 ORDER BY c DESC",
            scores_table, mods_query_str,
        ))
        .bind(mode)
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?
    };

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_BEATMAP_TASKS));

    let mut futures = FuturesUnordered::new();

    log::info!(
        beatmaps = beatmap_md5s.len(),
        mode = mode,
        rx = rx;
        "Starting beatmap recalculation"
    );

    let mut beatmaps_processed = 0;
    let total_beatmaps = beatmap_md5s.len();

    for (beatmap_md5,) in beatmap_md5s {
        let semaphore = semaphore.clone();
        let ctx = ctx.clone();
        let mods_query_str = mods_query_str.clone();

        let permit = semaphore.acquire_owned().await?;

        futures.push(tokio::spawn(async move {
            recalculate_beatmap(beatmap_md5, scores_table, mods_query_str, mode, rx, ctx).await?;
            beatmaps_processed += 1;

            drop(permit);

            if beatmaps_processed % 100 == 0 {
                log::info!(
                    beatmaps_left = total_beatmaps - beatmaps_processed as usize,
                    mode = mode,
                    rx = rx,
                    beatmaps_processed = beatmaps_processed;
                    "Beatmap recalculation progress",
                );
            }

            Ok::<(), anyhow::Error>(())
        }))
    }

    while let Some(result) = futures.next().await {
        if let Err(e) = result {
            log::error!(
                error = e.to_string();
                "Recalculating beatmap failed",
            );
        }
    }

    log::info!(
        mode = mode,
        rx = rx;
        "Beatmap recalculation finished"
    );

    Ok(())
}

fn calculate_new_pp(scores: &Vec<LightweightScore>, score_count: i32) -> i32 {
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
    mods_value: Option<i32>,
    neq_mods_value: Option<i32>,
    mapper_filter: Option<String>,
    map_filter: Option<Vec<i32>>,
) -> anyhow::Result<()> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let mods_query_str = if let Some(mods_value) = mods_value {
        format!("AND (mods & {}) > 0", mods_value)
    } else if let Some(neq_mods_value) = neq_mods_value {
        format!("AND (mods & {}) = 0", neq_mods_value)
    } else {
        "".to_string()
    };

    let beatmap_md5s: Vec<(String,)> = if let Some(mapper_filter) = mapper_filter {
        sqlx::query_as(
            &format!(
                "SELECT DISTINCT beatmap_md5 FROM {} INNER JOIN beatmaps USING(beatmap_md5) 
                WHERE userid = ? AND completed IN (2, 3) AND play_mode = ? AND beatmaps.file_name LIKE ? {}",
                scores_table, mods_query_str,
            )
        )
            .bind(user_id)
            .bind(mode)
            .bind(format!("%({mapper_filter})%"))
            .fetch_all(ctx.database.get().await?.deref_mut())
            .await?
    } else if let Some(map_filter) = map_filter {
        let formatted_beatmap_ids = format!(
            "({})",
            map_filter
                .iter()
                .map(|map| map.to_string())
                .collect::<Vec<String>>()
                .join(",")
        );
        sqlx::query_as(
            &format!(
                "SELECT DISTINCT beatmap_md5 FROM {} INNER JOIN beatmaps USING(beatmap_md5) 
                WHERE userid = ? AND completed IN (2, 3) AND play_mode = ? AND beatmaps.beatmap_id IN {} {}",
                scores_table, formatted_beatmap_ids, mods_query_str
            )
        )
            .bind(user_id)
            .bind(mode)
            .fetch_all(ctx.database.get().await?.deref_mut())
            .await?
    } else {
        sqlx::query_as(
            &format!(
                "SELECT DISTINCT beatmap_md5 FROM {} WHERE userid = ? AND completed IN (2, 3) AND play_mode = ? {}",
                scores_table, mods_query_str
            )
        )
            .bind(user_id)
            .bind(mode)
            .fetch_all(ctx.database.get().await?.deref_mut())
            .await?
    };

    for (beatmap_md5,) in beatmap_md5s {
        recalculate_status(user_id, mode, rx, beatmap_md5, ctx.clone()).await?;
    }

    Ok(())
}

async fn recalculate_user(
    user_id: i32,
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    mods_value: Option<i32>,
    neq_mods_value: Option<i32>,
    mapper_filter: Option<String>,
    map_filter: Option<Vec<i32>>,
) -> anyhow::Result<()> {
    recalculate_statuses(
        user_id,
        mode,
        rx,
        ctx.clone(),
        mods_value,
        neq_mods_value,
        mapper_filter,
        map_filter,
    )
    .await?;

    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let scores: Vec<LightweightScore> = sqlx::query_as(
        &format!(
            "SELECT s.id, s.mods, s.max_combo, s.play_mode, b.beatmap_id, s.pp, s.accuracy, s.misses_count
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

    let mut redis_connection = ctx.redis.get_multiplexed_async_connection().await?;

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

        let _: () = redis_connection
            .zadd(
                format!("ripple:{}:{}", redis_leaderboard, stats_prefix),
                user_id.to_string(),
                new_pp,
            )
            .await?;

        let _: () = redis_connection
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

    let _: () = redis_connection
        .publish("peppy:update_cached_stats", user_id)
        .await?;

    Ok(())
}

async fn recalculate_mode_users(
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    mods_value: Option<i32>,
    neq_mods_value: Option<i32>,
    mapper_filter: Option<String>,
    map_filter: Option<Vec<i32>>,
) -> anyhow::Result<()> {
    let user_ids: Vec<i32> = sqlx::query_scalar(&format!("SELECT id FROM users"))
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?;

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS));

    let mut users_recalculated = 0;

    for user_id_chunk in user_ids.chunks(BATCH_SIZE as usize) {
        let mut futures = FuturesUnordered::new();

        for &(user_id,) in user_id_chunk {
            let semaphore = semaphore.clone();
            let ctx = ctx.clone();
            let mapper_filter = mapper_filter.clone();
            let map_filter = map_filter.clone();

            let permit = semaphore.acquire_owned().await?;

            futures.push(tokio::spawn(async move {
                recalculate_user(
                    user_id,
                    mode,
                    rx,
                    ctx,
                    mods_value,
                    neq_mods_value,
                    mapper_filter,
                    map_filter,
                )
                .await?;
                drop(permit);
                Ok::<(), anyhow::Error>(())
            }))
        }

        while let Some(result) = futures.next().await {
            if let Err(e) = result {
                log::error!(
                    error = e.to_string();
                    "Processing user failed",
                );
            }
        }

        users_recalculated += BATCH_SIZE;

        log::info!(
            users_left = user_ids.len() - users_recalculated as usize,
            mode = mode,
            rx = rx,
            users_recalculated = users_recalculated;
            "Processed users",
        );
    }

    Ok(())
}

struct DeployArgs {
    modes: Vec<i32>,
    relax_bits: Vec<i32>,
    total_pp_only: bool,
    total_pp: bool,
    mods_filter: Option<i32>,
    neq_mods_filter: Option<i32>,
    mapper_filter: Option<String>,
    map_filter: Option<Vec<i32>>,
}

fn deploy_args_from_env() -> anyhow::Result<DeployArgs> {
    let modes_str = std::env::var("DEPLOY_MODES")?;
    let relax_bits_str = std::env::var("DEPLOY_RELAX_BITS")?;
    let total_pp_only_str = std::env::var("DEPLOY_TOTAL_PP_ONLY").unwrap_or("".to_string());
    let total_pp_str = std::env::var("DEPLOY_TOTAL_PP").unwrap_or("".to_string());
    let mods_filter_str = std::env::var("DEPLOY_MODS_FILTER").ok();
    let neq_mods_filter_str = std::env::var("DEPLOY_NEQ_MODS_FILTER").ok();
    let mapper_filter_str = std::env::var("DEPLOY_MAPPER_FILTER").ok();
    let map_filter_str = std::env::var("DEPLOY_MAP_FILTER").ok();

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
        total_pp: total_pp_str.to_lowercase().trim() == "1",
        mods_filter: mods_filter_str
            .map(|mods| mods.trim().parse::<i32>().expect("failed to parse mods")),
        neq_mods_filter: neq_mods_filter_str
            .map(|mods| mods.trim().parse::<i32>().expect("failed to parse mods")),
        mapper_filter: mapper_filter_str,
        map_filter: map_filter_str.map(|map_filter| {
            map_filter
                .trim()
                .split(',')
                .map(|map_filter| map_filter.parse::<i32>().expect("failed to parse map"))
                .collect::<Vec<_>>()
        }),
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

    print!("Total PP (y/n): ");
    std::io::stdout().flush()?;

    let mut total_str = String::new();
    std::io::stdin().read_line(&mut total_str)?;
    let total = total_str.to_lowercase().trim() == "y";

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

    print!("Neq mod value recalc only (y/n): ");
    std::io::stdout().flush()?;

    let mut neq_mod_recalc_value_only_str = String::new();
    std::io::stdin().read_line(&mut neq_mod_recalc_value_only_str)?;
    let neq_mod_recalc_value_only = neq_mod_recalc_value_only_str.to_lowercase().trim() == "y";

    print!("\n");
    std::io::stdout().flush()?;

    let mut neq_mods_value: Option<i32> = None;
    if neq_mod_recalc_value_only {
        print!("Neq mods value (int): ");
        std::io::stdout().flush()?;

        let mut neq_mods_value_str = String::new();
        std::io::stdin().read_line(&mut neq_mods_value_str)?;
        neq_mods_value = Some(
            neq_mods_value_str
                .trim()
                .parse::<i32>()
                .expect("failed to parse mods"),
        );

        print!("\n");
        std::io::stdout().flush()?;
    }

    print!("Mapper recalc only (y/n): ");
    std::io::stdout().flush()?;

    let mut mapper_recalc_only_str = String::new();
    std::io::stdin().read_line(&mut mapper_recalc_only_str)?;
    let mapper_recalc_only = mapper_recalc_only_str.to_lowercase().trim() == "y";

    print!("\n");
    std::io::stdout().flush()?;

    let mut mapper_filter: Option<String> = None;
    if mapper_recalc_only {
        print!("Mappers (comma delimited string): ");
        std::io::stdout().flush()?;

        let mut mapper_str = String::new();
        std::io::stdin().read_line(&mut mapper_str)?;
        mapper_filter = Some(mapper_str.trim().to_string());

        print!("\n");
        std::io::stdout().flush()?;
    }

    print!("Map recalc only (y/n): ");
    std::io::stdout().flush()?;

    let mut map_recalc_only_str = String::new();
    std::io::stdin().read_line(&mut map_recalc_only_str)?;
    let map_recalc_only = map_recalc_only_str.to_lowercase().trim() == "y";

    print!("\n");
    std::io::stdout().flush()?;

    let mut map_filter: Option<Vec<i32>> = None;
    if map_recalc_only {
        print!("Maps (comma delimited IDs): ");
        std::io::stdout().flush()?;

        let mut map_str = String::new();
        std::io::stdin().read_line(&mut map_str)?;
        map_filter = Some(
            map_str
                .trim()
                .split(',')
                .map(|s| s.parse::<i32>().expect("failed to parse map"))
                .collect::<Vec<_>>(),
        );

        print!("\n");
        std::io::stdout().flush()?;
    }

    Ok(DeployArgs {
        modes,
        relax_bits,
        total_pp_only: total_only,
        total_pp: total,
        mods_filter: mods_value,
        neq_mods_filter: neq_mods_value,
        mapper_filter,
        map_filter,
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
                        deploy_args.mods_filter,
                        deploy_args.neq_mods_filter,
                        deploy_args.mapper_filter.clone(),
                        deploy_args.map_filter.clone(),
                    )
                    .await?;
                }
            } else {
                recalculate_mode_scores(
                    mode,
                    0,
                    context_arc.clone(),
                    deploy_args.mods_filter,
                    deploy_args.neq_mods_filter,
                    deploy_args.mapper_filter.clone(),
                    deploy_args.map_filter.clone(),
                )
                .await?;
            }
        }
    }

    if !deploy_args.total_pp {
        return Ok(());
    }

    for mode in &deploy_args.modes {
        let mode = mode.clone();

        let rx = vec![0, 1, 2].contains(&mode);
        let ap = mode == 0;

        if rx || ap {
            for rx in &deploy_args.relax_bits {
                recalculate_mode_users(
                    mode,
                    rx.clone(),
                    context_arc.clone(),
                    deploy_args.mods_filter,
                    deploy_args.neq_mods_filter,
                    deploy_args.mapper_filter.clone(),
                    deploy_args.map_filter.clone(),
                )
                .await?;
            }
        } else {
            recalculate_mode_users(
                mode,
                0,
                context_arc.clone(),
                deploy_args.mods_filter,
                deploy_args.neq_mods_filter,
                deploy_args.mapper_filter.clone(),
                deploy_args.map_filter.clone(),
            )
            .await?;
        }
    }

    Ok(())
}
