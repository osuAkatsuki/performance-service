use crate::{context::Context, usecases};
use akatsuki_pp_rs::model::mode::GameMode;
use akatsuki_pp_rs::Beatmap;
use anyhow::anyhow;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use redis::AsyncCommands;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::Write;
use std::{ops::DerefMut, sync::Arc, time::SystemTime};
use tokio::sync::{Mutex, Semaphore};

#[derive(Clone, sqlx::FromRow)]
struct LightweightScore {
    pub id: i64,
    #[sqlx(rename = "userid")]
    pub user_id: i32,
    pub beatmap_md5: String,
    pub mods: i32,
    pub max_combo: i32,
    pub play_mode: i32,
    pub beatmap_id: i32,
    pub pp: f32,
    pub accuracy: f32,
    pub completed: i32,

    #[sqlx(rename = "misses_count")]
    pub count_misses: i32,
}

#[derive(Clone, sqlx::FromRow)]
struct ScoreStatus {
    pub id: i64,
    pub pp: f32,
    pub completed: i32,
}

#[derive(Clone)]
struct RecalculationRun {
    dry_run: bool,
    dry_run_score_pp: Option<Arc<Mutex<HashMap<i64, f32>>>>,
}

impl RecalculationRun {
    fn new(dry_run: bool, track_score_pp: bool) -> Self {
        Self {
            dry_run,
            dry_run_score_pp: (dry_run && track_score_pp)
                .then(|| Arc::new(Mutex::new(HashMap::new()))),
        }
    }

    async fn record_score_pp(&self, score_id: i64, pp: f32) {
        if let Some(score_pp) = &self.dry_run_score_pp {
            score_pp.lock().await.insert(score_id, pp);
        }
    }

    async fn score_pp(&self, score_id: i64) -> Option<f32> {
        let Some(score_pp) = &self.dry_run_score_pp else {
            return None;
        };

        score_pp.lock().await.get(&score_id).copied()
    }
}

fn round(x: f32, decimals: u32) -> f32 {
    let y = 10i32.pow(decimals) as f32;
    (x * y).round() / y
}

const MAX_CONCURRENT_BEATMAP_TASKS: usize = 10;
const MAX_CONCURRENT_TASKS: usize = 100;
const BATCH_SIZE: u32 = 1000;
const MAX_DRY_RUN_TRACKED_SCORE_PPS: i64 = 100_000;

#[derive(Clone, Default)]
struct DeployFilters {
    mods_filter: Option<i32>,
    neq_mods_filter: Option<i32>,
    mapper_filter: Option<String>,
    map_filter: Option<Vec<i32>>,
    pp_zero: bool,
    after_time: Option<i32>,
}

impl DeployFilters {
    fn score_column(alias: Option<&str>, column: &str) -> String {
        if let Some(alias) = alias {
            format!("{alias}.{column}")
        } else {
            column.to_string()
        }
    }

    fn score_conditions(&self, alias: Option<&str>) -> String {
        let mut conditions = Vec::new();

        if let Some(mods_value) = self.mods_filter {
            conditions.push(format!(
                "({} & {}) > 0",
                Self::score_column(alias, "mods"),
                mods_value
            ));
        } else if let Some(neq_mods_value) = self.neq_mods_filter {
            conditions.push(format!(
                "({} & {}) = 0",
                Self::score_column(alias, "mods"),
                neq_mods_value
            ));
        }

        if self.pp_zero {
            conditions.push(format!("{} = 0", Self::score_column(alias, "pp")));
        }

        if let Some(after_time) = self.after_time {
            conditions.push(format!(
                "{} >= {}",
                Self::score_column(alias, "time"),
                after_time
            ));
        }

        if conditions.is_empty() {
            "".to_string()
        } else {
            format!("AND {}", conditions.join(" AND "))
        }
    }

    fn score_selection_is_targeted(&self) -> bool {
        self.pp_zero || self.after_time.is_some()
    }

    fn status_filters(&self) -> Self {
        let mut filters = self.clone();

        // If pp=0 scores were just repaired, filtering status recalculation by
        // pp=0 would miss the beatmaps that need best-score repair.
        filters.pp_zero = false;

        filters
    }
}

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

async fn write_score_pp(
    score: &LightweightScore,
    new_pp: f64,
    scores_table: &str,
    rx: i32,
    ctx: Arc<Context>,
    run: &RecalculationRun,
) -> anyhow::Result<()> {
    let new_pp_for_stats = new_pp as f32;

    if run.dry_run {
        run.record_score_pp(score.id, new_pp_for_stats).await;
        log::info!(
            score_id = score.id,
            user_id = score.user_id,
            beatmap_md5 = score.beatmap_md5.as_str(),
            beatmap_id = score.beatmap_id,
            scores_table = scores_table,
            mode = score.play_mode,
            rx = rx,
            mods = score.mods,
            old_pp = score.pp,
            new_pp = new_pp,
            changed = score.pp != new_pp_for_stats;
            "Dry run would update score pp",
        );

        return Ok(());
    }

    let result = sqlx::query(&format!("UPDATE {} SET pp = ? WHERE id = ?", scores_table))
        .bind(new_pp)
        .bind(score.id)
        .execute(ctx.database.get().await?.deref_mut())
        .await?;

    log::info!(
        score_id = score.id,
        user_id = score.user_id,
        beatmap_md5 = score.beatmap_md5.as_str(),
        beatmap_id = score.beatmap_id,
        scores_table = scores_table,
        mode = score.play_mode,
        rx = rx,
        mods = score.mods,
        old_pp = score.pp,
        new_pp = new_pp,
        changed = score.pp != new_pp_for_stats,
        rows_affected = result.rows_affected();
        "Updated score pp",
    );

    Ok(())
}

async fn recalculate_relax_scores(
    scores: Vec<LightweightScore>,
    mods: i32,
    scores_table: &str,
    beatmap: &Beatmap,
    ctx: Arc<Context>,
    rx: i32,
    run: &RecalculationRun,
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

        write_score_pp(&score, pp as f64, scores_table, rx, ctx.clone(), run).await?;
    }

    Ok(())
}

async fn recalculate_scores(
    mut scores: Vec<LightweightScore>,
    scores_table: &str,
    beatmap: &Beatmap,
    ctx: Arc<Context>,
    rx: i32,
    run: &RecalculationRun,
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

    write_score_pp(
        &first_score,
        result.pp(),
        scores_table,
        rx,
        ctx.clone(),
        run,
    )
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

        write_score_pp(score, pp as f64, scores_table, rx, ctx.clone(), run).await?;
    }

    Ok(())
}

async fn recalculate_beatmap(
    beatmap_md5: String,
    scores_table: &str,
    filters: DeployFilters,
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    run: RecalculationRun,
) -> anyhow::Result<()> {
    let score_conditions = filters.score_conditions(Some("s"));

    let scores: Vec<LightweightScore> = sqlx::query_as(&format!(
        "SELECT s.id, s.userid, s.beatmap_md5, s.mods, s.max_combo, s.play_mode, b.beatmap_id, s.pp, s.accuracy, s.completed, s.misses_count
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
        scores_table, score_conditions,
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
            recalculate_relax_scores(
                mod_scores,
                mods,
                scores_table,
                &beatmap,
                ctx.clone(),
                rx,
                &run,
            )
            .await?;
        } else {
            recalculate_scores(mod_scores, scores_table, &beatmap, ctx.clone(), rx, &run).await?;
        }
    }

    log::info!(
        beatmap_id = base_score.beatmap_id,
        score_count = score_count,
        mode = mode,
        rx = rx,
        dry_run = run.dry_run;
        "Recalculated beatmap"
    );

    Ok(())
}

async fn recalculate_mode_scores(
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    filters: &DeployFilters,
    run: RecalculationRun,
) -> anyhow::Result<()> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let score_conditions = filters.score_conditions(Some("s"));

    let beatmap_md5s: Vec<(String,)> = if let Some(mapper_filter) = &filters.mapper_filter {
        sqlx::query_as(&format!(
            "SELECT s.beatmap_md5, COUNT(*) AS c FROM {} s INNER JOIN beatmaps b USING(beatmap_md5)
            WHERE s.completed IN (2, 3) AND s.play_mode = ? {} AND b.file_name LIKE ? GROUP BY s.beatmap_md5 ORDER BY c DESC",
            scores_table, score_conditions,
        ))
        .bind(mode)
        .bind(format!("%({mapper_filter})%"))
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?
    } else if let Some(map_filter) = &filters.map_filter {
        let formatted_beatmap_ids = format!(
            "({})",
            map_filter
                .iter()
                .map(|map| map.to_string())
                .collect::<Vec<String>>()
                .join(",")
        );
        sqlx::query_as(&format!(
            "SELECT s.beatmap_md5, COUNT(*) AS c FROM {} s INNER JOIN beatmaps b USING(beatmap_md5)
            WHERE s.completed IN (2, 3) AND s.play_mode = ? {} AND b.beatmap_id IN {} GROUP BY s.beatmap_md5 ORDER BY c DESC",
            scores_table, score_conditions, formatted_beatmap_ids
        ))
        .bind(mode)
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT s.beatmap_md5, COUNT(*) AS c FROM {} s WHERE s.completed IN (2, 3) AND s.play_mode = ? {} GROUP BY s.beatmap_md5 ORDER BY c DESC",
            scores_table, score_conditions,
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
        rx = rx,
        dry_run = run.dry_run;
        "Starting beatmap recalculation"
    );

    let mut beatmaps_processed = 0;
    let total_beatmaps = beatmap_md5s.len();

    for (beatmap_md5,) in beatmap_md5s {
        let semaphore = semaphore.clone();
        let ctx = ctx.clone();
        let filters = filters.clone();
        let run = run.clone();
        let dry_run = run.dry_run;

        let permit = semaphore.acquire_owned().await?;

        futures.push(tokio::spawn(async move {
            recalculate_beatmap(beatmap_md5, scores_table, filters, mode, rx, ctx, run).await?;
            beatmaps_processed += 1;

            drop(permit);

            if beatmaps_processed % 100 == 0 {
                log::info!(
                    beatmaps_left = total_beatmaps - beatmaps_processed as usize,
                    mode = mode,
                    rx = rx,
                    dry_run = dry_run,
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
        rx = rx,
        dry_run = run.dry_run;
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
    run: &RecalculationRun,
) -> anyhow::Result<Vec<(i64, i32)>> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let mut scores: Vec<ScoreStatus> = sqlx::query_as(
        &format!(
            "SELECT id, pp, completed FROM {} WHERE userid = ? AND play_mode = ? AND beatmap_md5 = ? AND completed IN (2, 3) ORDER BY pp DESC",
            scores_table
        )
    )
    .bind(user_id)
    .bind(mode)
    .bind(&beatmap_md5)
    .fetch_all(ctx.database.get().await?.deref_mut())
    .await?;

    if run.dry_run {
        for score in &mut scores {
            if let Some(planned_pp) = run.score_pp(score.id).await {
                score.pp = planned_pp;
            }
        }

        scores.sort_by(|a, b| b.pp.partial_cmp(&a.pp).unwrap_or(Ordering::Equal));
    }

    let best_id = scores[0].id;
    let non_bests = scores[1..].to_vec();
    let mut planned_completed = Vec::new();

    write_score_completed(
        scores_table,
        user_id,
        mode,
        rx,
        beatmap_md5.as_str(),
        best_id,
        scores[0].completed,
        3,
        ctx.clone(),
        run,
    )
    .await?;
    planned_completed.push((best_id, 3));

    for non_best in non_bests {
        write_score_completed(
            scores_table,
            user_id,
            mode,
            rx,
            beatmap_md5.as_str(),
            non_best.id,
            non_best.completed,
            2,
            ctx.clone(),
            run,
        )
        .await?;
        planned_completed.push((non_best.id, 2));
    }

    Ok(planned_completed)
}

async fn write_score_completed(
    scores_table: &str,
    user_id: i32,
    mode: i32,
    rx: i32,
    beatmap_md5: &str,
    score_id: i64,
    old_completed: i32,
    new_completed: i32,
    ctx: Arc<Context>,
    run: &RecalculationRun,
) -> anyhow::Result<()> {
    if run.dry_run {
        log::info!(
            score_id = score_id,
            user_id = user_id,
            beatmap_md5 = beatmap_md5,
            scores_table = scores_table,
            mode = mode,
            rx = rx,
            old_completed = old_completed,
            new_completed = new_completed,
            changed = old_completed != new_completed;
            "Dry run would update score status",
        );

        return Ok(());
    }

    let result = sqlx::query(&format!(
        "UPDATE {} SET completed = ? WHERE id = ?",
        scores_table
    ))
    .bind(new_completed)
    .bind(score_id)
    .execute(ctx.database.get().await?.deref_mut())
    .await?;

    log::info!(
        score_id = score_id,
        user_id = user_id,
        beatmap_md5 = beatmap_md5,
        scores_table = scores_table,
        mode = mode,
        rx = rx,
        old_completed = old_completed,
        new_completed = new_completed,
        changed = old_completed != new_completed,
        rows_affected = result.rows_affected();
        "Updated score status",
    );

    Ok(())
}

async fn recalculate_statuses(
    user_id: i32,
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    filters: &DeployFilters,
    run: &RecalculationRun,
) -> anyhow::Result<HashMap<i64, i32>> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let score_conditions = filters.score_conditions(Some("s"));

    let beatmap_md5s: Vec<(String,)> = if let Some(mapper_filter) = &filters.mapper_filter {
        sqlx::query_as(
            &format!(
                "SELECT DISTINCT s.beatmap_md5 FROM {} s INNER JOIN beatmaps b USING(beatmap_md5)
                WHERE s.userid = ? AND s.completed IN (2, 3) AND s.play_mode = ? AND b.file_name LIKE ? {}",
                scores_table, score_conditions,
            )
        )
            .bind(user_id)
            .bind(mode)
            .bind(format!("%({mapper_filter})%"))
            .fetch_all(ctx.database.get().await?.deref_mut())
            .await?
    } else if let Some(map_filter) = &filters.map_filter {
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
                "SELECT DISTINCT s.beatmap_md5 FROM {} s INNER JOIN beatmaps b USING(beatmap_md5)
                WHERE s.userid = ? AND s.completed IN (2, 3) AND s.play_mode = ? AND b.beatmap_id IN {} {}",
                scores_table, formatted_beatmap_ids, score_conditions
            )
        )
            .bind(user_id)
            .bind(mode)
            .fetch_all(ctx.database.get().await?.deref_mut())
            .await?
    } else {
        sqlx::query_as(
            &format!(
                "SELECT DISTINCT s.beatmap_md5 FROM {} s WHERE s.userid = ? AND s.completed IN (2, 3) AND s.play_mode = ? {}",
                scores_table, score_conditions
            )
        )
            .bind(user_id)
            .bind(mode)
            .fetch_all(ctx.database.get().await?.deref_mut())
            .await?
    };

    let mut planned_completed = HashMap::new();

    for (beatmap_md5,) in beatmap_md5s {
        for (score_id, completed) in
            recalculate_status(user_id, mode, rx, beatmap_md5, ctx.clone(), run).await?
        {
            if run.dry_run {
                planned_completed.insert(score_id, completed);
            }
        }
    }

    Ok(planned_completed)
}

async fn recalculate_user(
    user_id: i32,
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    filters: &DeployFilters,
    run: &RecalculationRun,
) -> anyhow::Result<()> {
    let planned_completed =
        recalculate_statuses(user_id, mode, rx, ctx.clone(), filters, run).await?;

    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let (scores, score_count) = if run.dry_run {
        let mut scores: Vec<LightweightScore> = sqlx::query_as(
            &format!(
                "SELECT s.id, s.userid, s.beatmap_md5, s.mods, s.max_combo, s.play_mode, b.beatmap_id, s.pp, s.accuracy, s.completed, s.misses_count
                FROM {} s
                INNER JOIN
                    beatmaps b
                    USING(beatmap_md5)
                WHERE
                    userid = ?
                    AND completed IN (2, 3)
                    AND play_mode = ?
                    AND ranked IN (3, 2)",
                scores_table
            )
        )
        .bind(user_id)
        .bind(mode)
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?;

        for score in &mut scores {
            if let Some(planned_pp) = run.score_pp(score.id).await {
                score.pp = planned_pp;
            }

            if let Some(planned_completed) = planned_completed.get(&score.id) {
                score.completed = *planned_completed;
            }
        }

        let mut best_scores = scores
            .into_iter()
            .filter(|score| score.completed == 3)
            .collect::<Vec<_>>();
        best_scores.sort_by(|a, b| b.pp.partial_cmp(&a.pp).unwrap_or(Ordering::Equal));

        let score_count = best_scores.len() as i32;
        best_scores.truncate(100);

        (best_scores, score_count)
    } else {
        let scores: Vec<LightweightScore> = sqlx::query_as(
            &format!(
                "SELECT s.id, s.userid, s.beatmap_md5, s.mods, s.max_combo, s.play_mode, b.beatmap_id, s.pp, s.accuracy, s.completed, s.misses_count
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

        (scores, score_count)
    };

    let new_pp = calculate_new_pp(&scores, score_count);

    let stats_mode = mode + (4 * rx);
    if run.dry_run {
        log::info!(
            user_id = user_id,
            mode = mode,
            rx = rx,
            stats_mode = stats_mode,
            new_pp = new_pp,
            score_count = score_count;
            "Dry run would update user total pp",
        );
    } else {
        let result = sqlx::query(&format!(
            "UPDATE user_stats SET pp = ? WHERE user_id = ? AND mode = ?",
        ))
        .bind(new_pp)
        .bind(user_id)
        .bind(stats_mode)
        .execute(ctx.database.get().await?.deref_mut())
        .await?;

        log::info!(
            user_id = user_id,
            mode = mode,
            rx = rx,
            stats_mode = stats_mode,
            new_pp = new_pp,
            score_count = score_count,
            rows_affected = result.rows_affected();
            "Updated user total pp",
        );
    }

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

        let global_key = format!("ripple:{}:{}", redis_leaderboard, stats_prefix);
        let country_key = format!(
            "ripple:{}:{}:{}",
            redis_leaderboard,
            stats_prefix,
            country.to_lowercase()
        );

        if run.dry_run {
            log::info!(
                user_id = user_id,
                mode = mode,
                rx = rx,
                redis_key = global_key.as_str(),
                new_pp = new_pp;
                "Dry run would update Redis leaderboard",
            );
            log::info!(
                user_id = user_id,
                mode = mode,
                rx = rx,
                redis_key = country_key.as_str(),
                new_pp = new_pp;
                "Dry run would update Redis leaderboard",
            );
        } else {
            let mut redis_connection = ctx.redis.get_multiplexed_async_connection().await?;

            let _: () = redis_connection
                .zadd(global_key.as_str(), user_id.to_string(), new_pp)
                .await?;
            log::info!(
                user_id = user_id,
                mode = mode,
                rx = rx,
                redis_key = global_key.as_str(),
                new_pp = new_pp;
                "Updated Redis leaderboard",
            );

            let _: () = redis_connection
                .zadd(country_key.as_str(), user_id.to_string(), new_pp)
                .await?;
            log::info!(
                user_id = user_id,
                mode = mode,
                rx = rx,
                redis_key = country_key.as_str(),
                new_pp = new_pp;
                "Updated Redis leaderboard",
            );
        }
    }

    if run.dry_run {
        log::info!(
            user_id = user_id,
            mode = mode,
            rx = rx;
            "Dry run would publish cached stats update",
        );
    } else {
        let mut redis_connection = ctx.redis.get_multiplexed_async_connection().await?;
        let _: () = redis_connection
            .publish("peppy:update_cached_stats", user_id)
            .await?;
        log::info!(
            user_id = user_id,
            mode = mode,
            rx = rx;
            "Published cached stats update",
        );
    }

    Ok(())
}

async fn recalculate_mode_users(
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    filters: &DeployFilters,
    affected_user_ids: Option<Vec<i32>>,
    run: RecalculationRun,
) -> anyhow::Result<()> {
    let user_ids: Vec<i32> = if let Some(user_ids) = affected_user_ids {
        user_ids
    } else {
        sqlx::query_scalar(&format!("SELECT id FROM users"))
            .fetch_all(ctx.database.get().await?.deref_mut())
            .await?
    };
    let status_filters = filters.status_filters();

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS));

    let mut users_recalculated = 0usize;

    for user_id_chunk in user_ids.chunks(BATCH_SIZE as usize) {
        let mut futures = FuturesUnordered::new();

        for &user_id in user_id_chunk {
            let semaphore = semaphore.clone();
            let ctx = ctx.clone();
            let filters = status_filters.clone();
            let run = run.clone();

            let permit = semaphore.acquire_owned().await?;

            futures.push(tokio::spawn(async move {
                recalculate_user(user_id, mode, rx, ctx, &filters, &run).await?;
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

        users_recalculated += user_id_chunk.len();

        log::info!(
            users_left = user_ids.len().saturating_sub(users_recalculated),
            mode = mode,
            rx = rx,
            dry_run = run.dry_run,
            users_recalculated = users_recalculated;
            "Processed users",
        );
    }

    Ok(())
}

async fn find_affected_user_ids(
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    filters: &DeployFilters,
) -> anyhow::Result<Vec<i32>> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };
    let score_conditions = filters.score_conditions(Some("s"));

    let user_ids: Vec<i32> = if let Some(mapper_filter) = &filters.mapper_filter {
        sqlx::query_scalar(&format!(
            "SELECT DISTINCT s.userid FROM {} s INNER JOIN beatmaps b USING(beatmap_md5)
            WHERE s.completed IN (2, 3) AND s.play_mode = ? {} AND b.file_name LIKE ?",
            scores_table, score_conditions,
        ))
        .bind(mode)
        .bind(format!("%({mapper_filter})%"))
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?
    } else if let Some(map_filter) = &filters.map_filter {
        let formatted_beatmap_ids = format!(
            "({})",
            map_filter
                .iter()
                .map(|map| map.to_string())
                .collect::<Vec<String>>()
                .join(",")
        );
        sqlx::query_scalar(&format!(
            "SELECT DISTINCT s.userid FROM {} s INNER JOIN beatmaps b USING(beatmap_md5)
            WHERE s.completed IN (2, 3) AND s.play_mode = ? {} AND b.beatmap_id IN {}",
            scores_table, score_conditions, formatted_beatmap_ids
        ))
        .bind(mode)
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?
    } else {
        sqlx::query_scalar(&format!(
            "SELECT DISTINCT s.userid FROM {} s
            WHERE s.completed IN (2, 3) AND s.play_mode = ? {}",
            scores_table, score_conditions,
        ))
        .bind(mode)
        .fetch_all(ctx.database.get().await?.deref_mut())
        .await?
    };

    Ok(user_ids)
}

async fn count_matching_scores_and_beatmaps(
    mode: i32,
    rx: i32,
    ctx: Arc<Context>,
    filters: &DeployFilters,
) -> anyhow::Result<(i64, i64)> {
    let scores_table = match rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };
    let score_conditions = filters.score_conditions(Some("s"));

    let counts: (i64, i64) = if let Some(mapper_filter) = &filters.mapper_filter {
        sqlx::query_as(&format!(
            "SELECT COUNT(s.id), COUNT(DISTINCT s.beatmap_md5)
            FROM {} s INNER JOIN beatmaps b USING(beatmap_md5)
            WHERE s.completed IN (2, 3) AND s.play_mode = ? {} AND b.file_name LIKE ?",
            scores_table, score_conditions,
        ))
        .bind(mode)
        .bind(format!("%({mapper_filter})%"))
        .fetch_one(ctx.database.get().await?.deref_mut())
        .await?
    } else if let Some(map_filter) = &filters.map_filter {
        let formatted_beatmap_ids = format!(
            "({})",
            map_filter
                .iter()
                .map(|map| map.to_string())
                .collect::<Vec<String>>()
                .join(",")
        );
        sqlx::query_as(&format!(
            "SELECT COUNT(s.id), COUNT(DISTINCT s.beatmap_md5)
            FROM {} s INNER JOIN beatmaps b USING(beatmap_md5)
            WHERE s.completed IN (2, 3) AND s.play_mode = ? {} AND b.beatmap_id IN {}",
            scores_table, score_conditions, formatted_beatmap_ids
        ))
        .bind(mode)
        .fetch_one(ctx.database.get().await?.deref_mut())
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT COUNT(s.id), COUNT(DISTINCT s.beatmap_md5)
            FROM {} s
            WHERE s.completed IN (2, 3) AND s.play_mode = ? {}",
            scores_table, score_conditions,
        ))
        .bind(mode)
        .fetch_one(ctx.database.get().await?.deref_mut())
        .await?
    };

    Ok(counts)
}

async fn count_all_users(ctx: Arc<Context>) -> anyhow::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(ctx.database.get().await?.deref_mut())
        .await
        .map_err(Into::into)
}

fn recalculation_scopes(deploy_args: &DeployArgs) -> Vec<(i32, i32)> {
    let mut scopes = Vec::new();

    for mode in &deploy_args.modes {
        let mode = *mode;
        let rx = vec![0, 1, 2].contains(&mode);
        let ap = mode == 0;

        if rx || ap {
            for rx in &deploy_args.relax_bits {
                scopes.push((mode, *rx));
            }
        } else {
            scopes.push((mode, 0));
        }
    }

    scopes
}

async fn preview_recalculation(deploy_args: &DeployArgs, ctx: Arc<Context>) -> anyhow::Result<()> {
    for (mode, rx) in recalculation_scopes(deploy_args) {
        let (matching_scores, matching_beatmaps) =
            count_matching_scores_and_beatmaps(mode, rx, ctx.clone(), &deploy_args.filters).await?;
        let affected_users = if deploy_args.filters.score_selection_is_targeted() {
            find_affected_user_ids(mode, rx, ctx.clone(), &deploy_args.filters)
                .await?
                .len() as i64
        } else if deploy_args.total_pp {
            count_all_users(ctx.clone()).await?
        } else {
            0
        };

        log::info!(
            mode = mode,
            rx = rx,
            score_recalculation = !deploy_args.total_pp_only,
            total_pp_recalculation = deploy_args.total_pp,
            matching_scores = matching_scores,
            matching_beatmaps = matching_beatmaps,
            affected_users = affected_users;
            "Preview recalculation scope",
        );
    }

    log::info!(
        "Preview complete; no score, status, stats, leaderboard, or cache work was performed"
    );

    Ok(())
}

fn dry_run_tracks_score_pp(deploy_args: &DeployArgs) -> bool {
    deploy_args.dry_run && deploy_args.total_pp && !deploy_args.total_pp_only
}

async fn validate_dry_run_tracking(
    deploy_args: &DeployArgs,
    ctx: Arc<Context>,
) -> anyhow::Result<()> {
    if !dry_run_tracks_score_pp(deploy_args) {
        return Ok(());
    }

    let mut matching_scores = 0;
    for (mode, rx) in recalculation_scopes(deploy_args) {
        let (scope_scores, _) =
            count_matching_scores_and_beatmaps(mode, rx, ctx.clone(), &deploy_args.filters).await?;
        matching_scores += scope_scores;
    }

    if matching_scores > MAX_DRY_RUN_TRACKED_SCORE_PPS {
        return Err(anyhow!(
            "DEPLOY_DRY_RUN would need to retain {matching_scores} planned score PP values to simulate user totals; limit is {MAX_DRY_RUN_TRACKED_SCORE_PPS}. Use DEPLOY_PREVIEW=1, narrow the score filters, or dry-run the score and total-PP phases separately."
        ));
    }

    log::info!(
        matching_scores = matching_scores,
        max_tracked_score_pp = MAX_DRY_RUN_TRACKED_SCORE_PPS;
        "Dry run will retain planned score PP values for user-total simulation",
    );

    Ok(())
}

struct DeployArgs {
    modes: Vec<i32>,
    relax_bits: Vec<i32>,
    total_pp_only: bool,
    total_pp: bool,
    preview: bool,
    dry_run: bool,
    filters: DeployFilters,
}

fn deploy_after_time_from_env() -> anyhow::Result<Option<i32>> {
    let after_time_str = std::env::var("DEPLOY_AFTER_TIME").ok();
    let after_date_str = std::env::var("DEPLOY_AFTER_DATE").ok();

    if after_time_str.is_some() && after_date_str.is_some() {
        return Err(anyhow!(
            "DEPLOY_AFTER_TIME and DEPLOY_AFTER_DATE cannot both be set"
        ));
    }

    if let Some(after_time_str) = after_time_str {
        return after_time_str
            .trim()
            .parse::<i32>()
            .map(Some)
            .map_err(|_| anyhow!("failed to parse DEPLOY_AFTER_TIME"));
    }

    if let Some(after_date_str) = after_date_str {
        let after_date = chrono::NaiveDate::parse_from_str(after_date_str.trim(), "%Y-%m-%d")
            .map_err(|_| anyhow!("failed to parse DEPLOY_AFTER_DATE as YYYY-MM-DD"))?;
        let after_time = after_date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("failed to build DEPLOY_AFTER_DATE timestamp"))?
            .and_utc()
            .timestamp();
        let after_time = after_time
            .try_into()
            .map_err(|_| anyhow!("DEPLOY_AFTER_DATE is outside the supported timestamp range"))?;

        return Ok(Some(after_time));
    }

    Ok(None)
}

fn deploy_args_from_env() -> anyhow::Result<DeployArgs> {
    let modes_str = std::env::var("DEPLOY_MODES")?;
    let relax_bits_str = std::env::var("DEPLOY_RELAX_BITS")?;
    let total_pp_only_str = std::env::var("DEPLOY_TOTAL_PP_ONLY").unwrap_or("".to_string());
    let total_pp_str = std::env::var("DEPLOY_TOTAL_PP").unwrap_or("".to_string());
    let preview = std::env::var("DEPLOY_PREVIEW")
        .unwrap_or_default()
        .to_lowercase()
        .trim()
        == "1";
    let dry_run = std::env::var("DEPLOY_DRY_RUN")
        .unwrap_or_default()
        .to_lowercase()
        .trim()
        == "1";
    let mods_filter_str = std::env::var("DEPLOY_MODS_FILTER").ok();
    let neq_mods_filter_str = std::env::var("DEPLOY_NEQ_MODS_FILTER").ok();
    let mapper_filter_str = std::env::var("DEPLOY_MAPPER_FILTER").ok();
    let map_filter_str = std::env::var("DEPLOY_MAP_FILTER").ok();
    let pp_zero = std::env::var("DEPLOY_PP_ZERO")
        .unwrap_or_default()
        .to_lowercase()
        .trim()
        == "1";
    let after_time = deploy_after_time_from_env()?;

    if preview && dry_run {
        return Err(anyhow!(
            "DEPLOY_PREVIEW and DEPLOY_DRY_RUN cannot both be set"
        ));
    }

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
        preview,
        dry_run,
        filters: DeployFilters {
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
            pp_zero,
            after_time,
        },
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
        preview: false,
        dry_run: false,
        filters: DeployFilters {
            mods_filter: mods_value,
            neq_mods_filter: neq_mods_value,
            mapper_filter,
            map_filter,
            ..Default::default()
        },
    })
}

fn retrieve_deploy_args() -> anyhow::Result<DeployArgs> {
    let env_requested = std::env::vars().any(|(key, _)| key.starts_with("DEPLOY_"));

    if env_requested {
        deploy_args_from_env()
    } else {
        deploy_args_from_input()
    }
}

pub async fn serve(context: Context) -> anyhow::Result<()> {
    let deploy_args = retrieve_deploy_args()?;

    let context_arc = Arc::new(context);
    let mut affected_users_by_scope = HashMap::new();
    let run = RecalculationRun::new(deploy_args.dry_run, dry_run_tracks_score_pp(&deploy_args));

    if deploy_args.preview {
        preview_recalculation(&deploy_args, context_arc).await?;
        return Ok(());
    }

    validate_dry_run_tracking(&deploy_args, context_arc.clone()).await?;

    if run.dry_run {
        log::info!("Dry run started; score, status, stats, leaderboard, and cache writes will be logged but not performed");
    }

    if !deploy_args.total_pp_only {
        for mode in &deploy_args.modes {
            let mode = mode.clone();

            let rx = vec![0, 1, 2].contains(&mode);
            let ap = mode == 0;

            if rx || ap {
                for rx in &deploy_args.relax_bits {
                    if deploy_args.filters.score_selection_is_targeted() {
                        let affected_user_ids = find_affected_user_ids(
                            mode,
                            *rx,
                            context_arc.clone(),
                            &deploy_args.filters,
                        )
                        .await?;
                        affected_users_by_scope.insert((mode, *rx), affected_user_ids);
                    }

                    recalculate_mode_scores(
                        mode,
                        rx.clone(),
                        context_arc.clone(),
                        &deploy_args.filters,
                        run.clone(),
                    )
                    .await?;
                }
            } else {
                if deploy_args.filters.score_selection_is_targeted() {
                    let affected_user_ids =
                        find_affected_user_ids(mode, 0, context_arc.clone(), &deploy_args.filters)
                            .await?;
                    affected_users_by_scope.insert((mode, 0), affected_user_ids);
                }

                recalculate_mode_scores(
                    mode,
                    0,
                    context_arc.clone(),
                    &deploy_args.filters,
                    run.clone(),
                )
                .await?;
            }
        }
    }

    if !deploy_args.total_pp {
        if run.dry_run {
            log::info!("Dry run complete; no score, status, stats, leaderboard, or cache writes were performed");
        }
        return Ok(());
    }

    for mode in &deploy_args.modes {
        let mode = mode.clone();

        let rx = vec![0, 1, 2].contains(&mode);
        let ap = mode == 0;

        if rx || ap {
            for rx in &deploy_args.relax_bits {
                let affected_user_ids = if deploy_args.filters.score_selection_is_targeted() {
                    if let Some(affected_user_ids) = affected_users_by_scope.get(&(mode, *rx)) {
                        Some(affected_user_ids.clone())
                    } else {
                        Some(
                            find_affected_user_ids(
                                mode,
                                *rx,
                                context_arc.clone(),
                                &deploy_args.filters,
                            )
                            .await?,
                        )
                    }
                } else {
                    None
                };

                recalculate_mode_users(
                    mode,
                    rx.clone(),
                    context_arc.clone(),
                    &deploy_args.filters,
                    affected_user_ids,
                    run.clone(),
                )
                .await?;
            }
        } else {
            let affected_user_ids = if deploy_args.filters.score_selection_is_targeted() {
                if let Some(affected_user_ids) = affected_users_by_scope.get(&(mode, 0)) {
                    Some(affected_user_ids.clone())
                } else {
                    Some(
                        find_affected_user_ids(mode, 0, context_arc.clone(), &deploy_args.filters)
                            .await?,
                    )
                }
            } else {
                None
            };

            recalculate_mode_users(
                mode,
                0,
                context_arc.clone(),
                &deploy_args.filters,
                affected_user_ids,
                run.clone(),
            )
            .await?;
        }
    }

    if run.dry_run {
        log::info!("Dry run complete; no score, status, stats, leaderboard, or cache writes were performed");
    }

    Ok(())
}
