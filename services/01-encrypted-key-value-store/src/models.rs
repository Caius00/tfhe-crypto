use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct PutRequest {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub session_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetRequest {
    pub key: Vec<u8>,
    pub session_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ExistsRequest {
    pub key: Vec<u8>,
    pub session_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DeleteRequest {
    pub key: Vec<u8>,
    pub session_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ValueResponse {
    pub value: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateSessionRequest {
    pub server_key: Vec<u8>,
}

#[derive(Debug)]
pub enum AppError {
    Redis(redis::RedisError),
    Json(serde_json::Error),
    NotFound(String),
    Unauthorized,
    InternalError(String),
    // ValueLength(usize),
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
                "Missing or invalid session ID".into(),
            ),
            AppError::InternalError(e) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal Server Error: {}", e),
            ),
            // AppError::ValueLength(len) => (
            //     axum::http::StatusCode::BAD_REQUEST,
            //     format!(
            //         "Value length ({}) doesnt match fixed length: {}",
            //         len, VALUE_LENGTH
            //     ),
            // ),
        };

        (status, axum::Json(MessageResponse { message })).into_response()
    }
}

impl aide::OperationOutput for AppError {
    type Inner = MessageResponse;
}
