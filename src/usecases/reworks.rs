use crate::{
    context::Context,
    errors::{Error, ErrorCode},
    models::rework::Rework,
    repositories,
};
use std::sync::Arc;

pub async fn fetch_one(rework_id: i32, context: Arc<Context>) -> Result<Rework, Error> {
    let repo = repositories::reworks::ReworksRepository::new(context);
    let rework = repo.fetch_one(rework_id).await.map_err(|_| Error {
        error_code: ErrorCode::InternalServerError,
        user_feedback: "Failed to fetch rework",
    })?;

    match rework {
        Some(rework) => Ok(rework),
        None => Err(Error {
            error_code: ErrorCode::NotFound,
            user_feedback: "Rework not found",
        }),
    }
}

pub async fn fetch_all(context: Arc<Context>) -> Result<Vec<Rework>, Error> {
    let repo = repositories::reworks::ReworksRepository::new(context);
    let reworks = repo.fetch_all().await.map_err(|_| Error {
        error_code: ErrorCode::InternalServerError,
        user_feedback: "Failed to fetch reworks",
    })?;

    Ok(reworks)
}
