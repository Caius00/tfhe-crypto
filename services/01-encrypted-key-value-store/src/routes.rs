use axum::extract::{State};
use axum::Json;
use base64::Engine;
use base64::engine::general_purpose;
use tfhe::{set_server_key, CompressedServerKey};
use uuid::Uuid;
use crate::custom_fhe_ascii_string::{CompressedCustomFheAsciiString, CustomFheAsciiString};
use crate::models::{AppError, CreateSessionRequest, DeleteRequest, ExistsRequest, GetRequest, MessageResponse, PutRequest, ValueResponse};
use crate::store::SharedState;

async fn set_route_server_key(state: &SharedState, session_id: &str) {
    let keys_lock = state.server_keys.read().await;
    let server_key = keys_lock.get(session_id).unwrap();
    set_server_key(server_key.clone());
}

pub async fn create_session_route(
    State(state): State<SharedState>,
    body: Json<CreateSessionRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    // TODO() remove base64 encoding
    let decoded_server_key = general_purpose::STANDARD.decode(&body.server_key).unwrap();
    let compressed_server_key: CompressedServerKey = bincode::deserialize(&decoded_server_key).unwrap();
    let server_key = compressed_server_key.decompress();

    let session_id = Uuid::new_v4().to_string();
    state.server_keys.write().await.insert(session_id.clone(), server_key);

    Ok(Json(MessageResponse {
        message: session_id
    }))
}

/// TODO() Use compression?
pub async fn put_route (
    State(state): State<SharedState>,
    Json(body): Json<PutRequest>
) -> Result<(), AppError> {
    let parsed_key = CompressedCustomFheAsciiString::new(body.key);
    let parsed_value = CompressedCustomFheAsciiString::new(body.value);

    let decompressed_key = parsed_key.decompress();
    let decompressed_value = parsed_value.decompress();
    state.put(&decompressed_key, &decompressed_value).await;

    Ok(())
}

pub async fn get_route (
    State(state): State<SharedState>,
    Json(body): Json<GetRequest>
) -> Result<Json<ValueResponse>, AppError> {
    set_route_server_key(&state, &body.session_id).await;

    let compressed_key = CompressedCustomFheAsciiString::new(body.key);
    let decompressed_key = compressed_key.decompress();

    let (value, found_value) = state.get(&decompressed_key).await.unwrap();

    Ok(Json(ValueResponse {
        value: value.compress().string
    }))
}

pub async fn exists_route (
    State(state): State<SharedState>,
    Json(body): Json<ExistsRequest>
) -> Result<Json<ValueResponse>, AppError> {
    set_route_server_key(&state, &body.session_id).await;

    let compressed_key = CompressedCustomFheAsciiString::new(body.key);
    let decompressed_key = compressed_key.decompress();

    let exists = state.exists(&decompressed_key).await.unwrap();
    let compressed = exists.compress();
    let serialized = bincode::serialize(&compressed).unwrap();

    Ok(Json(ValueResponse {
        value: serialized
    }))
}

pub async fn delete_route (
    State(state): State<SharedState>,
    Json(body): Json<DeleteRequest>
) -> Result<(), AppError> {
    set_route_server_key(&state, &body.session_id).await;
    let parsed_key = CompressedCustomFheAsciiString::new(body.key).decompress();

    state.delete(&parsed_key).await.unwrap();

    Ok(())
}

pub async fn clear_db (
    State(state): State<SharedState>,
) -> Result<Json<MessageResponse>, AppError> {
    let mut con = state.client.get_multiplexed_async_connection().await?;
    redis::cmd("FLUSHDB").query_async::<_, ()>(&mut con).await?;

    Ok(Json(MessageResponse {
        message: "Cleared DB Successful".to_string()
    }))
}
