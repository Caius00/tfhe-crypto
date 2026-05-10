use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use crate::models::{AppError, PutKeyRequest, MessageResponse, KeyValueResponse, KeyListResponse, ExistsResponse};
use crate::store::SharedState;

/// Extract user ID from the `X-User-Id` header.
/// TODO() Maybe change to JWT?
fn get_user_id(headers: &HeaderMap) -> Result<String, AppError> {
    headers
        .get("X-User-Id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or(AppError::Unauthorized)
}

pub async fn put_entry (
    State(state): State<SharedState>,
    header: HeaderMap,
    Json(body): Json<PutKeyRequest>
) -> Result<Json<MessageResponse>, AppError> {
    let user_hash = get_user_id(&header)?;

    state
        .set(&user_hash, &body.key, &body.value, body.ttl)
        .await?;

    Ok(Json(MessageResponse {
        message: format!("Key {} put successfully", body.key)
    }))
}
pub async fn get_entry (
    State(state): State<SharedState>,
    header: HeaderMap,
    Path(key): Path<String>,
) -> Result<Json<KeyValueResponse>, AppError> {
    let user_hash = get_user_id(&header)?;
    let value = state.get(&user_hash, &key).await?;

    match value {
        Some(value) => Ok(Json(KeyValueResponse { key, value })),
        None => Err(AppError::NotFound(key))
    }
}

pub async fn delete_entry (
    State(state): State<SharedState>,
    header: HeaderMap,
    Path(key): Path<String>,
) -> Result<Json<MessageResponse>, AppError> {
    let user_hash = get_user_id(&header)?;
    let deleted = state.delete(&user_hash, &key).await?;

    if deleted {
        Ok(Json(MessageResponse {
            message: format!("Key '{}' deleted", key),
        }))
    } else {
        Err(AppError::NotFound(key))
    }
}

pub async fn exists (
    State(state): State<SharedState>,
    header: HeaderMap,
    Path(key): Path<String>,
) -> Result<Json<ExistsResponse>, AppError> {
    let user_hash = get_user_id(&header)?;
    let exists = state.exists(&user_hash, &key).await?;

    Ok(Json(ExistsResponse {
        exists
    }))
}

/// Extra
pub async fn list_keys (
    State(state): State<SharedState>,
    header: HeaderMap,
) -> Result<Json<KeyListResponse>, AppError> {
    let user_hash = get_user_id(&header)?;

    let keys = state
        .list_entries(&user_hash)
        .await?;

    Ok(Json(KeyListResponse {
        keys
    }))
}

/// Extra
pub async fn delete_all (
    State(state): State<SharedState>,
    header: HeaderMap,
) -> Result<Json<MessageResponse>, AppError> {
    let user_hash = get_user_id(&header)?;

    state
        .delete_all(&user_hash)
        .await?;

    Ok(Json(MessageResponse {
        message: format!("Deleted all entries for: \"{}\"", user_hash)
    }))
}
