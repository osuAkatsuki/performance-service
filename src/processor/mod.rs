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

use aim_accuracy_fix::Beatmap as AimAccuracyFixBeatmap;
use fix_inconsistent_powers::Beatmap as FixInconsistentPowersBeatmap;
use flashlight_hotfix::Beatmap as FlashlightHotfixBeatmap;
use improved_miss_penalty::Beatmap as ImprovedMissPenaltyBeatmap;
use remove_accuracy_pp::Beatmap as RemoveAccuracyBeatmap;
use remove_manual_adjustments::Beatmap as RemoveManualAdjustmentsBeatmap;
use stream_nerf_speed_value::Beatmap as StreamNerfSpeedValueBeatmap;

fn round(x: f32, decimals: u32) -> f32 {
    let y = 10i32.pow(decimals) as f32;
    (x * y).round() / y
}

async fn calculate_improved_miss_penalty_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = ImprovedMissPenaltyBeatmap::from_bytes(&beatmap_bytes).await?;

    let result = improved_miss_penalty::osu_2019::OsuPP::new(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as usize)
        .n300(score.count_300 as usize)
        .n100(score.count_100 as usize)
        .n50(score.count_50 as usize)
        .misses(score.count_misses as usize)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_flashlight_hotfix_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = FlashlightHotfixBeatmap::from_bytes(&beatmap_bytes).await?;

    let result = flashlight_hotfix::osu_2019::OsuPP::new(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as usize)
        .n300(score.count_300 as usize)
        .n100(score.count_100 as usize)
        .n50(score.count_50 as usize)
        .misses(score.count_misses as usize)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_remove_accuracy_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = RemoveAccuracyBeatmap::from_bytes(&beatmap_bytes).await?;

    let result = remove_accuracy_pp::osu_2019::OsuPP::new(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as usize)
        .n300(score.count_300 as usize)
        .n100(score.count_100 as usize)
        .n50(score.count_50 as usize)
        .misses(score.count_misses as usize)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_stream_nerf_speed_value_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = StreamNerfSpeedValueBeatmap::from_bytes(&beatmap_bytes).await?;

    let result = stream_nerf_speed_value::osu_2019::OsuPP::new(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as usize)
        .n300(score.count_300 as usize)
        .n100(score.count_100 as usize)
        .n50(score.count_50 as usize)
        .misses(score.count_misses as usize)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_remove_manual_adjustments_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = RemoveManualAdjustmentsBeatmap::from_bytes(&beatmap_bytes).await?;

    let result = remove_manual_adjustments::osu_2019::OsuPP::new(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as usize)
        .n300(score.count_300 as usize)
        .n100(score.count_100 as usize)
        .n50(score.count_50 as usize)
        .misses(score.count_misses as usize)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_fix_inconsistent_powers_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = FixInconsistentPowersBeatmap::from_bytes(&beatmap_bytes).await?;

    let result = fix_inconsistent_powers::osu_2019::OsuPP::new(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as usize)
        .n300(score.count_300 as usize)
        .n100(score.count_100 as usize)
        .n50(score.count_50 as usize)
        .misses(score.count_misses as usize)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_aim_accuracy_fix_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = AimAccuracyFixBeatmap::from_bytes(&beatmap_bytes)?;

    let result = aim_accuracy_fix::osu_2019::OsuPP::from_map(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as u32)
        .n300(score.count_300 as u32)
        .n100(score.count_100 as u32)
        .n50(score.count_50 as u32)
        .misses(score.count_misses as u32)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_improved_miss_penalty_and_acc_rework_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = improved_miss_penalty_and_acc_rework::Beatmap::from_bytes(&beatmap_bytes)?;

    let result = improved_miss_penalty_and_acc_rework::osu_2019::OsuPP::from_map(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as u32)
        .n300(score.count_300 as u32)
        .n100(score.count_100 as u32)
        .n50(score.count_50 as u32)
        .misses(score.count_misses as u32)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
    if pp.is_infinite() || pp.is_nan() {
        pp = 0.0;
    }

    Ok(pp)
}

async fn calculate_everything_at_once_pp(
    score: &RippleScore,
    context: Arc<Context>,
) -> anyhow::Result<f32> {
    let beatmap_bytes =
        usecases::beatmaps::fetch_beatmap_osu_file(score.beatmap_id, context).await?;
    let beatmap = everything_at_once::Beatmap::from_bytes(&beatmap_bytes)?;

    let result = everything_at_once::osu_2019::OsuPP::from_map(&beatmap)
        .mods(score.mods as u32)
        .combo(score.max_combo as u32)
        .n300(score.count_300 as u32)
        .n100(score.count_100 as u32)
        .n50(score.count_50 as u32)
        .misses(score.count_misses as u32)
        .calculate();

    let mut pp = round(result.pp as f32, 2);
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
            19 => calculate_improved_miss_penalty_pp(score, context.clone()).await?,
            21 => calculate_flashlight_hotfix_pp(score, context.clone()).await?,
            22 => calculate_remove_accuracy_pp(score, context.clone()).await?,
            23 => calculate_stream_nerf_speed_value_pp(score, context.clone()).await?,
            24 => calculate_remove_manual_adjustments_pp(score, context.clone()).await?,
            25 => calculate_fix_inconsistent_powers_pp(score, context.clone()).await?,
            26 => calculate_aim_accuracy_fix_pp(score, context.clone()).await?,
            27 => calculate_improved_miss_penalty_and_acc_rework_pp(score, context.clone()).await?,
            28 => calculate_everything_at_once_pp(score, context.clone()).await?,
            _ => unreachable!(),
        };

        log::info!(
            score_id = score.id;
            "Recalculated PP for score",
        );

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
    total_pp += 416.6667 * (1.0 - 0.995_f32.powi(score_count as i32));

    total_pp.round() as i32
}

async fn handle_queue_request(
    request: QueueRequest,
    context: Arc<Context>,
    delivery_tag: u64,
) -> anyhow::Result<()> {
    let Some(rework) = usecases::reworks::fetch_one(request.rework_id, context.clone()).await? else {
        anyhow::bail!("failed to find rework");
    };
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
            "SELECT COUNT(s.id) FROM {} s INNER JOIN beatmaps USING(beatmap_md5) WHERE userid = ? AND completed = 3 AND play_mode = ? AND ranked IN (3, 2) LIMIT 1000",
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

    let old_pp: u32 =
        sqlx::query_scalar(r#"SELECT pp FROM user_stats WHERE user_id = ? AND mode = ?"#)
            .bind(request.user_id)
            .bind(rework.mode + (rework.rx * 4))
            .fetch_one(context.database.get().await?.deref_mut())
            .await?;

    let rework_stats = ReworkStats {
        user_id: request.user_id,
        rework_id: rework.rework_id,
        old_pp: old_pp as i32,
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

    let mut redis_connection = context.redis.get_multiplexed_async_connection().await?;
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
        user_id = request.user_id,
        rework_name = rework.rework_name;
        "Processed recalculation for user on rework",
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

            let result = handle_queue_request(
                deserialized_data,
                context.clone(),
                delivery.delivery_tag.clone(),
            )
            .await;

            if let Err(e) = result {
                log::error!(error = e.to_string(); "Error processing queue request");
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
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
