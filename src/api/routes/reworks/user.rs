use redis::AsyncCommands;
use std::{ops::DerefMut, sync::Arc};

use axum::{
    extract::{Extension, Path},
    routing::get,
    Json, Router,
};

use crate::{
    api::error::AppResult,
    context::Context,
    models::{
        rework::Rework,
        stats::{APIReworkStats, ReworkStats},
        user::ReworkUser,
    },
};

pub fn router() -> Router {
    Router::new()
        .route("/api/v1/reworks/users/:user_id", get(get_rework_user))
        .route(
            "/api/v1/reworks/:rework_id/users/:user_id/stats",
            get(get_rework_stats),
        )
}

async fn get_rework_user(
    ctx: Extension<Arc<Context>>,
    Path(user_id): Path<i32>,
) -> AppResult<Json<Option<ReworkUser>>> {
    let user_info: Option<(String, String)> =
        sqlx::query_as("SELECT username, country FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(ctx.database.get().await?.deref_mut())
            .await?;

    if user_info.is_none() {
        return Ok(Json(None));
    }

    let (user_name, country) = user_info.unwrap();

    let rework_stats: Vec<ReworkStats> =
        sqlx::query_as("SELECT * FROM rework_stats WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(ctx.database.get().await?.deref_mut())
            .await?;

    let rework_ids = rework_stats
        .iter()
        .map(|stat| stat.rework_id)
        .collect::<Vec<i32>>();

    let mut reworks: Vec<Rework> = Vec::new();
    for rework_id in rework_ids {
        let rework: Option<Rework> = sqlx::query_as("SELECT * FROM reworks WHERE rework_id = ?")
            .bind(rework_id)
            .fetch_optional(ctx.database.get().await?.deref_mut())
            .await?;
        if let Some(rework) = rework {
            reworks.push(rework);
        } else {
            continue;
        }
    }

    Ok(Json(Some(ReworkUser {
        user_id,
        user_name,
        country,
        reworks,
    })))
}

async fn get_rework_stats(
    ctx: Extension<Arc<Context>>,
    Path((rework_id, user_id)): Path<(i32, i32)>,
) -> AppResult<Json<Option<APIReworkStats>>> {
    let stats: Option<ReworkStats> = sqlx::query_as(
        "SELECT user_id, rework_id, old_pp, new_pp FROM rework_stats WHERE user_id = ? AND rework_id = ?"
    )
        .bind(user_id)
        .bind(rework_id)
        .fetch_optional(ctx.database.get().await?.deref_mut())
        .await?;

    if stats.is_none() {
        return Ok(Json(None));
    }

    let stats = stats.unwrap();

    let user_info: (String, String) =
        sqlx::query_as("SELECT username, country FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(ctx.database.get().await?.deref_mut())
            .await?;

    let mut redis_connection = ctx.redis.get_async_connection().await?;

    let rework: Rework = sqlx::query_as("SELECT * FROM reworks WHERE rework_id = ?")
        .bind(rework_id)
        .fetch_one(ctx.database.get().await?.deref_mut())
        .await?;

    let redis_leaderboard = match rework.rx {
        0 => "leaderboard".to_string(),
        1 => "relaxboard".to_string(),
        2 => "autoboard".to_string(),
        _ => unreachable!(),
    };

    let stats_prefix = match rework.mode {
        0 => "std",
        1 => "taiko",
        2 => "ctb",
        3 => "mania",
        _ => unreachable!(),
    };

    let old_rank_idx: Option<i64> = redis_connection
        .zrevrank(
            format!("ripple:{}:{}", redis_leaderboard, stats_prefix),
            user_id,
        )
        .await?;

    let new_rank_idx: Option<i64> = redis_connection
        .zrevrank(format!("rework:leaderboard:{}", rework.rework_id), user_id)
        .await?;

    let api_user = APIReworkStats::from_stats(
        stats,
        user_info.0,
        user_info.1,
        (old_rank_idx.unwrap_or(-1) + 1) as u64,
        (new_rank_idx.unwrap_or(-1) + 1) as u64,
    );
    Ok(Json(Some(api_user)))
}
