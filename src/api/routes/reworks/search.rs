use axum::{
    extract::{Extension, Json, Path, Query},
    routing::get,
    Router,
};
use std::sync::Arc;

use crate::{
    api::error::{ApiError, AppResult},
    context::Context,
    errors::{Error, ErrorCode},
};

pub fn router() -> Router {
    Router::new().route("/api/v1/reworks/:rework_id/users/search", get(search_users))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SearchQuery {
    query: String,
}

#[derive(serde::Serialize, serde::Deserialize, sqlx::FromRow)]
struct SearchUser {
    user_id: i32,
    user_name: String,
}

async fn search_users(
    ctx: Extension<Arc<Context>>,
    Path(rework_id): Path<i32>,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Vec<SearchUser>>> {
    let users: Vec<SearchUser> = sqlx::query_as(
        "SELECT id user_id, username user_name FROM users WHERE username_safe LIKE ?",
    )
    .bind(format!(
        "%{}%",
        query
            .query
            .to_lowercase()
            .replace(" ", "_")
            .replace(|c: char| !c.is_ascii(), "")
    ))
    .fetch_all(&ctx.database)
    .await
    .map_err(|_| {
        ApiError(Error {
            error_code: ErrorCode::InternalServerError,
            user_feedback: "Failed to fetch users",
        })
    })?;

    let mut to_remove: Vec<i32> = Vec::new();
    for user in &users {
        let in_rework: bool =
            sqlx::query_scalar("SELECT 1 FROM rework_stats WHERE user_id = ? AND rework_id = ?")
                .bind(user.user_id)
                .bind(rework_id)
                .fetch_optional(&ctx.database)
                .await
                .map_err(|_| {
                    ApiError(Error {
                        error_code: ErrorCode::InternalServerError,
                        user_feedback: "Failed to check if user is in rework",
                    })
                })?
                .unwrap_or(false);

        if !in_rework {
            to_remove.push(user.user_id);
        }
    }

    let mut users: Vec<SearchUser> = users
        .iter()
        .filter(|user| !to_remove.contains(&user.user_id))
        .map(|user| SearchUser {
            user_id: user.user_id,
            user_name: user.user_name.clone(),
        })
        .collect();

    users.sort_by(|user1, user2| {
        strsim::jaro(&user1.user_name, &query.query)
            .partial_cmp(&strsim::jaro(&user2.user_name, &query.query))
            .unwrap()
    });

    Ok(Json(users))
}
