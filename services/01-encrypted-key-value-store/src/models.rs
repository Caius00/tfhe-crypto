use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PutKeyRequest {
    pub key: String,
    pub value: String,
    pub ttl: u64, // TODO() make it server only instead
}

#[derive(Debug, Serialize)]
pub struct KeyValueResponse {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct ExistsResponse {
    pub exists: bool,
}

#[derive(Debug, Serialize)]
pub struct KeyListResponse {
    pub keys: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug)]
pub enum AppError {
    Redis(redis::RedisError),
    Json(serde_json::Error),
    NotFound(String),
    Unauthorized,
}

impl From<redis::RedisError> for AppError {
    fn from(e: redis::RedisError) -> Self {
        AppError::Redis(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Json(e)
    }
}

// Convert AppError into axum HTTP responses
impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AppError::Redis(e) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Redis error: {}", e),
            ),
            AppError::Json(e) => (
                axum::http::StatusCode::BAD_REQUEST,
                format!("JSON error: {}", e),
            ),
            AppError::NotFound(key) => (
                axum::http::StatusCode::NOT_FOUND,
                format!("Key not found: {}", key),
            ),
            AppError::Unauthorized => (
                axum::http::StatusCode::UNAUTHORIZED,
                "Missing or invalid user ID".into(),
            ),
        };

        (status, axum::Json(MessageResponse { message })).into_response()
    }
}
