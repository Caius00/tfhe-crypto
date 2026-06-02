use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct PutRequest {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct GetRequest {
    pub key: Vec<u8>,
    pub session_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct ExistsRequest {
    pub key: Vec<u8>,
    pub session_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct DeleteRequest {
    pub key: Vec<u8>,
    pub session_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct ValueResponse {
    pub value: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub server_key: String,
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
