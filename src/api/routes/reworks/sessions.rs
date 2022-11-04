use axum::{
    extract::{Extension, Path},
    routing::{delete, post},
    Json, Router,
};
use std::sync::Arc;

use crate::context::Context;
use crate::usecases;

pub fn router() -> Router {
    Router::new()
        .route("/api/v1/reworks/sessions", post(create_session))
        .route("/api/v1/reworks/sessions/:session", delete(delete_session))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CreateSessionRequest {
    pub username: String,
    pub password_md5: String,
}

async fn create_session(
    Extension(ctx): Extension<Arc<Context>>,
    Json(request): Json<CreateSessionRequest>,
) -> Json<usecases::sessions::CreateSessionResponse> {
    let response =
        usecases::sessions::create(request.username, request.password_md5, ctx.clone()).await;

    Json(response)
}

async fn delete_session(Extension(ctx): Extension<Arc<Context>>, Path(session): Path<String>) {
    let _ = usecases::sessions::delete(session, ctx.clone())
        .await
        .unwrap();
}
