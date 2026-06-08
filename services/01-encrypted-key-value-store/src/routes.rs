use crate::custom_fhe_ascii_string::CompressedCustomFheAsciiString;
use crate::models::{
    AppError, CreateSessionRequest, DeleteRequest, ExistsRequest, GetRequest, MessageResponse,
    PutRequest, ValueResponse,
};
use crate::store::{SharedState};
use axum::extract::State;
use axum::Json;
use tfhe::{set_server_key, CompressedServerKey};
use uuid::Uuid;

pub const VALUE_LENGTH: usize = 200;

async fn set_route_server_key(state: &SharedState, session_id: &str) -> Result<(), AppError> {
    let keys_lock = state.server_keys.read().await;
    let server_key = keys_lock.get(session_id).ok_or(AppError::Unauthorized)?;

    set_server_key(server_key.clone());

    Ok(())
}

pub async fn create_session_route(
    State(state): State<SharedState>,
    body: Json<CreateSessionRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    let compressed_server_key: CompressedServerKey =
        bincode::deserialize(&body.server_key).unwrap();
    let server_key = compressed_server_key.decompress();

    let session_id = Uuid::new_v4().to_string();
    state
        .server_keys
        .write()
        .await
        .insert(session_id.clone(), server_key);

    Ok(Json(MessageResponse {
        message: session_id,
    }))
}

pub async fn put_route(
    State(state): State<SharedState>,
    Json(body): Json<PutRequest>,
) -> Result<(), AppError> {
    let parsed_key = CompressedCustomFheAsciiString::new(body.key);
    let parsed_value = CompressedCustomFheAsciiString::new(body.value);

    let decompressed_key = parsed_key.decompress();
    let decompressed_value = parsed_value.decompress();

    // if decompressed_value.string.len() != VALUE_LENGTH {
    //     Err(AppError::ValueLength(decompressed_value.string.len()))?
    // }

    state
        .put(&decompressed_key, &decompressed_value, &body.session_id)
        .await?;

    Ok(())
}

pub async fn get_route(
    State(state): State<SharedState>,
    Json(body): Json<GetRequest>,
) -> Result<Json<ValueResponse>, AppError> {
    let server_key = {
        let keys_lock = state.server_keys.read().await;
        keys_lock.get(&body.session_id)
            .ok_or(AppError::Unauthorized)?
            .clone()
    };

    let compressed_key = CompressedCustomFheAsciiString::new(body.key);
    let decompressed_key = compressed_key.decompress();

    let (value, _) = state.get(decompressed_key, body.session_id).await?;

    Ok(Json(ValueResponse {
        value: value.compress().string,
    }))
}

pub async fn exists_route(
    State(state): State<SharedState>,
    Json(body): Json<ExistsRequest>,
) -> Result<Json<ValueResponse>, AppError> {
    let server_key = {
        let keys_lock = state.server_keys.read().await;
        keys_lock.get(&body.session_id)
            .ok_or(AppError::Unauthorized)?
            .clone()
    };

    let compressed_key = CompressedCustomFheAsciiString::new(body.key);
    let decompressed_key = compressed_key.decompress();

    let exists = state.exists(decompressed_key, body.session_id).await?;

    let compressed = exists.compress();
    let serialized = bincode::serialize(&compressed).unwrap();

    Ok(Json(ValueResponse { value: serialized }))
}

pub async fn delete_route(
    State(state): State<SharedState>,
    Json(body): Json<DeleteRequest>,
) -> Result<(), AppError> {
    let server_key = {
        let keys_lock = state.server_keys.read().await;
        keys_lock.get(&body.session_id)
            .ok_or(AppError::Unauthorized)?
            .clone()
    };

    let parsed_key = CompressedCustomFheAsciiString::new(body.key).decompress();

    state.delete(parsed_key, body.session_id, server_key).await?;

    Ok(())
}

pub async fn clear_db(State(state): State<SharedState>) -> Result<Json<MessageResponse>, AppError> {
    let mut con = state.client.get_multiplexed_async_connection().await?;
    redis::cmd("FLUSHDB").query_async::<_, ()>(&mut con).await?;

    Ok(Json(MessageResponse {
        message: "Cleared DB Successful".to_string(),
    }))
}
