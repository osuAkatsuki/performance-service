use std::{ops::DerefMut, sync::Arc, time::Duration};

use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions},
    types::FieldTable,
};
use redis::AsyncCommands;
use rkyv::Deserialize;
use tokio_stream::StreamExt;

use crate::{
    context::Context,
    models::{
        queue::QueueRequest,
        rework::Rework,
        score::{ReworkScore, RippleScore},
        stats::ReworkStats,
    },
    usecases,
};

use conceptual_rework::{
    Beatmap as ConceptualBeatmap, BeatmapExt as ConceptualBeatmapExt,
    GameMode as ConceptualGameMode,
};
use skill_rebalance::{
    Beatmap as SkillRebalanceBeatmap, BeatmapExt as SkillRebalanceBeatmapExt,
    GameMode as SkillRebalanceGameMode,
};

fn round(x: f32, decimals: u32) -> f32 {
    let y = 10i32.pow(decimals) as f32;
    (x * y).round() / y
}

async fn calculate_conceptual_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = ConceptualBeatmap::from_bytes(&beatmap_bytes).await?;

    let result = beatmap
        .pp()
        .mode(match score.play_mode {
            0 => ConceptualGameMode::Osu,
            1 => ConceptualGameMode::Taiko,
            2 => ConceptualGameMode::Catch,
            3 => ConceptualGameMode::Mania,
            _ => unreachable!(),
        })
        .mods(score.mods as u32)
        .combo(score.max_combo as usize)
        .accuracy(score.accuracy as f64)
        .n_misses(score.count_misses as usize)
        .calculate();

    let mut pp = round(result.pp() as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_skill_rebalance_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = SkillRebalanceBeatmap::from_bytes(&beatmap_bytes).await?;

    let result = beatmap
        .pp()
        .mode(match score.play_mode {
            0 => SkillRebalanceGameMode::Osu,
            1 => SkillRebalanceGameMode::Taiko,
            2 => SkillRebalanceGameMode::Catch,
            3 => SkillRebalanceGameMode::Mania,
            _ => unreachable!(),
        })
        .mods(score.mods as u32)
        .combo(score.max_combo as usize)
        .accuracy(score.accuracy as f64)
        .n_misses(score.count_misses as usize)
        .calculate();

    let mut pp = round(result.pp() as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn process_scores(
    rework: &Rework,
    scores: Vec<RippleScore>,
    context: Arc<Context>,
) -> anyhow::Result<Vec<ReworkScore>> {
    let mut rework_scores: Vec<ReworkScore> = Vec::new();

    for score in &scores {
        let new_pp = match rework.rework_id {
            10 => calculate_conceptual_pp(score, context.clone()).await?,
            11 => calculate_conceptual_pp(score, context.clone()).await?,
            12 => calculate_conceptual_pp(score, context.clone()).await?,
            13 => calculate_skill_rebalance_pp(score, context.clone()).await?,
            _ => unreachable!(),
        };

        log::info!("Recalculated PP for score ID {}", score.id);

        let rework_score = ReworkScore::from_ripple_score(score, rework.rework_id, new_pp);
        rework_scores.push(rework_score);
    }

    Ok(rework_scores)
}

fn calculate_new_pp(scores: &Vec<ReworkScore>, score_count: i32) -> i32 {
    let mut total_pp = 0.0;

    for (idx, score) in scores.iter().enumerate() {
        total_pp += score.new_pp * 0.95_f32.powi(idx as i32);
    }

    // bonus pp
    total_pp += 416.6667 * (1.0 - 0.9994_f32.powi(score_count as i32));

    total_pp.round() as i32
}

async fn handle_queue_request(
    request: QueueRequest,
    context: Arc<Context>,
    delivery_tag: u64,
) -> anyhow::Result<()> {
    let rework = usecases::reworks::fetch_one(request.rework_id, context.clone()).await?;
    if rework.is_none() {
        anyhow::bail!("failed to find rework");
    }

    let rework = rework.unwrap();
    let scores_table = match rework.rx {
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
    .bind(request.user_id)
    .bind(rework.mode)
    .fetch_all(context.database.get().await?.deref_mut())
    .await?;

    let score_count: i32 = sqlx::query_scalar(
        &format!(
            "SELECT COUNT(s.id) FROM {} s INNER JOIN beatmaps USING(beatmap_md5) WHERE userid = ? AND completed = 3 AND play_mode = ? AND ranked IN (3, 2) LIMIT 25397",
            scores_table
        )
    )
        .bind(request.user_id)
        .bind(rework.mode)
        .fetch_one(context.database.get().await?.deref_mut())
        .await?;

    let rework_scores = process_scores(&rework, scores, context.clone()).await?;
    let new_pp = calculate_new_pp(&rework_scores, score_count);

    for rework_score in rework_scores {
        sqlx::query(
            "REPLACE INTO rework_scores (score_id, beatmap_id, beatmapset_id, user_id, rework_id, max_combo, 
            mods, accuracy, score, num_300s, num_100s, num_50s, num_gekis, num_katus, num_misses, old_pp, new_pp) 
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(rework_score.score_id)
        .bind(rework_score.beatmap_id)
        .bind(rework_score.beatmapset_id)
        .bind(rework_score.user_id)
        .bind(rework_score.rework_id)
        .bind(rework_score.max_combo)
        .bind(rework_score.mods)
        .bind(rework_score.accuracy)
        .bind(rework_score.score)
        .bind(rework_score.num_300s)
        .bind(rework_score.num_100s)
        .bind(rework_score.num_50s)
        .bind(rework_score.num_gekis)
        .bind(rework_score.num_katus)
        .bind(rework_score.num_misses)
        .bind(rework_score.old_pp)
        .bind(rework_score.new_pp)
        .execute(context.database.get().await?.deref_mut())
        .await?;
    }

    let stats_table = match rework.rx {
        0 => "users_stats",
        1 => "rx_stats",
        2 => "ap_stats",
        _ => unreachable!(),
    };

    let stats_prefix = match rework.mode {
        0 => "std",
        1 => "taiko",
        2 => "ctb",
        3 => "mania",
        _ => unreachable!(),
    };

    let old_pp: i32 = sqlx::query_scalar(&format!(
        r#"SELECT pp_{} FROM {} WHERE id = ?"#,
        stats_prefix, stats_table
    ))
    .bind(request.user_id)
    .fetch_one(context.database.get().await?.deref_mut())
    .await?;

    let rework_stats = ReworkStats {
        user_id: request.user_id,
        rework_id: rework.rework_id,
        old_pp,
        new_pp,
    };

    sqlx::query(
        "REPLACE INTO rework_stats (user_id, rework_id, old_pp, new_pp) VALUES (?, ?, ?, ?)",
    )
    .bind(rework_stats.user_id)
    .bind(rework_stats.rework_id)
    .bind(rework_stats.old_pp)
    .bind(rework_stats.new_pp)
    .execute(context.database.get().await?.deref_mut())
    .await?;

    let mut redis_connection = context.redis.get_async_connection().await?;
    let _: () = redis_connection
        .zadd(
            format!("rework:leaderboard:{}", request.rework_id),
            request.user_id,
            rework_stats.new_pp,
        )
        .await?;

    sqlx::query("UPDATE rework_queue SET processed_at = CURRENT_TIMESTAMP() WHERE user_id = ? AND rework_id = ?")
        .bind(request.user_id)
        .bind(request.rework_id)
        .execute(context.database.get().await?.deref_mut())
        .await?;

    context
        .amqp_channel
        .basic_ack(delivery_tag, BasicAckOptions::default())
        .await?;

    log::info!(
        "Processed recalculation for user ID {} on rework {}",
        request.user_id,
        rework.rework_name
    );

    Ok(())
}

async fn rmq_listen(context: Arc<Context>) -> anyhow::Result<()> {
    context
        .amqp_channel
        .queue_declare(
            "rework_queue",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut consumer = context
        .amqp_channel
        .basic_consume(
            "rework_queue",
            "akatsuki-rework",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    while let Some(delivery) = consumer.next().await {
        if let Ok(delivery) = delivery {
            let deserialized_data: QueueRequest =
                rkyv::check_archived_root::<QueueRequest>(&delivery.data)
                    .expect("failed to check archived root?")
                    .deserialize(&mut rkyv::Infallible)?;

            log::info!(
                "Received recalculation request for user ID {} on rework ID {}",
                deserialized_data.user_id,
                deserialized_data.rework_id
            );

            let context_clone = context.clone();
            tokio::spawn(async move {
                let result = handle_queue_request(
                    deserialized_data,
                    context_clone,
                    delivery.delivery_tag.clone(),
                )
                .await;

                if result.is_err() {
                    panic!("Error processing queue request: {:?}", result);
                }
            });
        }
    }

    Ok(())
}

pub async fn serve(context: Context) -> anyhow::Result<()> {
    let mut retry_interval = tokio::time::interval(Duration::from_secs(5));
    let context_arc = Arc::new(context);

    loop {
        retry_interval.tick().await;
        rmq_listen(context_arc.clone()).await?;
    }
}
