use crate::errors::{Error, ErrorCode};
use axum::{body::Body, http::Response, response::IntoResponse};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ApiError(pub Error);

pub type AppResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    type Body = Body;
    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> Response<Self::Body> {
        Response::builder()
            .status(match self.0.error_code {
                ErrorCode::BadRequest => 400,
                ErrorCode::NotFound => 404,
                ErrorCode::DependencyFailed => 424,
                ErrorCode::InternalServerError => 500,
            })
            .body(Body::from(serde_json::to_string(&self.0).unwrap()))
            .unwrap()
    }
}
