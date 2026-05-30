use axum::extract::{State};
use axum::http::HeaderMap;
use axum::Json;
use rayon::prelude::*;
use crate::custom_fhe_ascii_string::CustomFheAsciiString;
use crate::models::{AppError, DeleteMultipleRequest, DeleteRequest, ExistsRequest, GetRequest, MessageResponse, PutRequest, ValueResponse};
use crate::store::SharedState;

/// Use compression?
pub async fn put_route (
    State(state): State<SharedState>,
    header: HeaderMap,
    Json(body): Json<PutRequest>
) -> Result<Json<MessageResponse>, AppError> {
    let parsed_key = CustomFheAsciiString::from(body.key);
    let parsed_value = CustomFheAsciiString::from(body.value);
    state.put(&parsed_key, &parsed_value).await;

    Ok(Json(MessageResponse {
        message: "Insertion Successful".to_string()
    }))
}

pub async fn get_route (
    State(state): State<SharedState>,
    header: HeaderMap,
    Json(body): Json<GetRequest>
) -> Result<Json<ValueResponse>, AppError> {
    let parsed_key = CustomFheAsciiString::from(body.key);
    let server_key = bincode::deserialize(&body.server_key).unwrap();

    let (value, found_value) = state.get(&parsed_key).await.unwrap();

    Ok(Json(ValueResponse {
        value: value.serialize().string
    }))
}

pub async fn exists_route (
    State(state): State<SharedState>,
    header: HeaderMap,
    Json(body): Json<ExistsRequest>
) -> Result<Json<ValueResponse>, AppError> {
    let parsed_key = CustomFheAsciiString::from(body.key);
    let server_key = bincode::deserialize(&body.server_key).unwrap();

    let exists = state.exists(&parsed_key).await.unwrap();
    let serialized = bincode::serialize(&exists).unwrap();

    Ok(Json(ValueResponse {
        value: serialized
    }))
}

pub async fn delete_route (
    State(state): State<SharedState>,
    header: HeaderMap,
    Json(body): Json<DeleteRequest>
) -> Result<Json<MessageResponse>, AppError> {
    let parsed_key = CustomFheAsciiString::from(body.key);
    let server_key = bincode::deserialize(&body.server_key).unwrap();

    state.delete(&parsed_key).await.unwrap();

    Ok(Json(MessageResponse {
        message: "Deletion Successful".to_string()
    }))
}

pub async fn delete_multiple_route (
    State(state): State<SharedState>,
    header: HeaderMap,
    Json(body): Json<DeleteMultipleRequest>
) -> Result<Json<MessageResponse>, AppError> {
    let parsed_keys = body.key
        .par_iter()
        .map(|k| CustomFheAsciiString::from(k))
        .collect::<Vec<CustomFheAsciiString>>();
    let server_key = bincode::deserialize(&body.server_key).unwrap();

    state.delete_multiple(&parsed_keys).await.unwrap();

    Ok(Json(MessageResponse {
        message: "Deletion Successful".to_string()
    }))
}
