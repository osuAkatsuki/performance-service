use std::sync::Arc;
use std::time::SystemTime;
use std::{io::Write, ops::DerefMut};

use crate::{
    context::Context,
    models::{queue::QueueRequest, rework::Rework},
    usecases,
};

use lapin::{options::BasicPublishOptions, BasicProperties};
use redis::AsyncCommands;

async fn queue_user(user_id: i32, rework: &Rework, context: &Context) -> anyhow::Result<()> {
    let scores_table = match rework.rx {
        0 => "scores",
        1 => "scores_relax",
        2 => "scores_ap",
        _ => unreachable!(),
    };

    let last_score_time: Option<i32> = sqlx::query_scalar(&format!(
        "SELECT max(time) FROM {} INNER JOIN beatmaps USING(beatmap_md5)
        WHERE userid = ? AND completed = 3 AND ranked IN (2, 3) AND play_mode = ?
        ORDER BY pp DESC LIMIT 100",
        scores_table
    ))
    .bind(user_id)
    .bind(rework.mode)
    .fetch_optional(context.database.get().await?.deref_mut())
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

    if inactive_days >= 60 {
        return Ok(());
    }

    let in_queue: Option<bool> = sqlx::query_scalar(
        "SELECT 1 FROM rework_queue WHERE user_id = ? AND rework_id = ? AND processed_at < ?",
    )
    .bind(user_id)
    .bind(rework.rework_id)
    .bind(rework.updated_at)
    .fetch_optional(context.database.get().await?.deref_mut())
    .await?;

    if in_queue.is_some() {
        return Ok(());
    }

    sqlx::query(r#"REPLACE INTO rework_queue (user_id, rework_id) VALUES (?, ?)"#)
        .bind(user_id)
        .bind(rework.rework_id)
        .execute(context.database.get().await?.deref_mut())
        .await?;

    context
        .amqp_channel
        .basic_publish(
            "",
            "rework_queue",
            BasicPublishOptions::default(),
            &rkyv::to_bytes::<_, 256>(&QueueRequest {
                user_id,
                rework_id: rework.rework_id,
            })?,
            BasicProperties::default(),
        )
        .await?;

    log::info!(
        user_id = user_id;
        "Queued user",
    );
    Ok(())
}

pub async fn serve(context: Context) -> anyhow::Result<()> {
    print!("Enter a rework ID to mass recalculate: ");
    std::io::stdout().flush()?;

    let mut rework_id_str = String::new();
    std::io::stdin().read_line(&mut rework_id_str)?;
    let rework_id = rework_id_str.trim().parse::<i32>()?;

    print!("\n");
    std::io::stdout().flush()?;

    log::info!(
        rework_id = rework_id;
        "Mass recalculating on rework",
    );

    let rework = usecases::reworks::fetch_one(rework_id, Arc::from(context.clone()))
        .await?
        .expect("failed to find rework");

    sqlx::query("DELETE FROM rework_scores WHERE rework_id = ?")
        .bind(rework_id)
        .execute(context.database.get().await?.deref_mut())
        .await?;

    sqlx::query("DELETE FROM rework_stats WHERE rework_id = ?")
        .bind(rework_id)
        .execute(context.database.get().await?.deref_mut())
        .await?;

    sqlx::query("DELETE FROM rework_queue WHERE rework_id = ?")
        .bind(rework_id)
        .execute(context.database.get().await?.deref_mut())
        .await?;

    let mut redis_connection = context.redis.get_async_connection().await?;
    let _: () = redis_connection
        .del(format!("rework:leaderboard:{}", rework_id))
        .await?;

    let user_ids: Vec<(i32,)> = sqlx::query_as(&format!(
        "SELECT users.id, pp
        FROM user_stats
        INNER JOIN users ON users.id = user_stats.user_id
        WHERE pp > 0 AND mode = ?
        AND users.privileges & 1
        ORDER BY pp DESC
        "
    ))
    .bind(rework.mode + (rework.rx * 4))
    .fetch_all(context.database.get().await?.deref_mut())
    .await?;

    for (user_id,) in user_ids {
        queue_user(user_id, &rework, &context).await?;
    }

    Ok(())
}
