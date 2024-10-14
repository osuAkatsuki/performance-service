use axum::{body::Body, http::Response, response::IntoResponse};

pub struct AppError();
pub type AppResult<T> = Result<T, AppError>;

impl IntoResponse for AppError {
    type Body = Body;
    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> Response<Self::Body> {
        Response::builder()
            .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("internal server error"))
            .unwrap()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(_err: E) -> Self {
        Self()
    }
}
