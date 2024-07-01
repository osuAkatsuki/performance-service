use std::io::Write;
use std::sync::Arc;

use crate::{
    context::Context,
    models::{queue::QueueRequest, rework::Rework},
    usecases,
};

use lapin::{options::BasicPublishOptions, BasicProperties};
use redis::AsyncCommands;

async fn queue_user(user_id: i32, rework: &Rework, context: &Context) -> anyhow::Result<()> {
    let in_queue: Option<bool> = sqlx::query_scalar(
        "SELECT 1 FROM rework_queue WHERE user_id = ? AND rework_id = ? AND processed_at < ?",
    )
    .bind(user_id)
    .bind(rework.rework_id)
    .bind(rework.updated_at)
    .fetch_optional(&context.database)
    .await?;

    if in_queue.is_some() {
        return Ok(());
    }

    sqlx::query(r#"REPLACE INTO rework_queue (user_id, rework_id) VALUES (?, ?)"#)
        .bind(user_id)
        .bind(rework.rework_id)
        .execute(&context.database)
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

    log::info!("Queued user ID {}", user_id);
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

    print!("Enter the user ID to recalculate: ");
    std::io::stdout().flush()?;

    let mut user_id_str = String::new();
    std::io::stdin().read_line(&mut user_id_str)?;
    let user_id = user_id_str.trim().parse::<i32>()?;

    print!("\n");
    std::io::stdout().flush()?;

    log::info!(
        "Mass recalculating on rework ID {} for {}",
        rework_id,
        user_id
    );

    let rework = match usecases::reworks::fetch_one(rework_id, Arc::from(context.clone())).await {
        Ok(rework) => rework,
        Err(e) => {
            log::error!("Failed to fetch rework: {:?}", e);
            return Ok(());
        }
    };

    sqlx::query("DELETE FROM rework_scores WHERE rework_id = ? AND user_id = ?")
        .bind(rework_id)
        .bind(user_id)
        .execute(&context.database)
        .await?;

    sqlx::query("DELETE FROM rework_stats WHERE rework_id = ? AND user_id = ?")
        .bind(rework_id)
        .bind(user_id)
        .execute(&context.database)
        .await?;

    sqlx::query("DELETE FROM rework_queue WHERE rework_id = ? AND user_id = ?")
        .bind(rework_id)
        .bind(user_id)
        .execute(&context.database)
        .await?;

    let mut redis_connection = context.redis.get_async_connection().await?;
    let _: () = redis_connection
        .zrem(format!("rework:leaderboard:{}", rework_id), user_id)
        .await?;

    queue_user(user_id, &rework, &context).await?;

    Ok(())
}
