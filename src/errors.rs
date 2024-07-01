#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum ErrorCode {
    BadRequest,
    NotFound,
    DependencyFailed,
    InternalServerError,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Error {
    pub error_code: ErrorCode,
    pub user_feedback: &'static str,
}

impl Error {
    pub fn new(error_code: ErrorCode, user_feedback: &'static str) -> Self {
        Self {
            error_code,
            user_feedback,
        }
    }
}
