use crate::custom_fhe_ascii_string::CompressedCustomFheAsciiString;
use crate::models::{
    AppError, CreateSessionRequest, DeleteRequest, ExistsRequest, GetRequest, MessageResponse,
    PutRequest, ValueResponse,
};
use crate::store::SharedState;
use axum::extract::State;
use axum::Json;
use tfhe::{CompressedServerKey};
use uuid::Uuid;

// pub const VALUE_LENGTH: usize = 200;

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

    state.put(parsed_key, parsed_value, body.session_id).await?;

    Ok(())
}

pub async fn get_route(
    State(state): State<SharedState>,
    Json(body): Json<GetRequest>,
) -> Result<Json<ValueResponse>, AppError> {
    let compressed_key = CompressedCustomFheAsciiString::new(body.key);

    let (value, _) = state.get(compressed_key, body.session_id).await?;

    Ok(Json(ValueResponse {
        value: value.compress().string,
    }))
}

pub async fn exists_route(
    State(state): State<SharedState>,
    Json(body): Json<ExistsRequest>,
) -> Result<Json<ValueResponse>, AppError> {
    let compressed_key = CompressedCustomFheAsciiString::new(body.key);

    let exists = state.exists(compressed_key, body.session_id).await?;

    let compressed = exists.compress();
    let serialized = bincode::serialize(&compressed).unwrap();

    Ok(Json(ValueResponse { value: serialized }))
}

pub async fn delete_route(
    State(state): State<SharedState>,
    Json(body): Json<DeleteRequest>,
) -> Result<(), AppError> {
    let parsed_key = CompressedCustomFheAsciiString::new(body.key);

    state.delete(parsed_key, body.session_id).await?;

    Ok(())
}

pub async fn clear_db(State(state): State<SharedState>) -> Result<Json<MessageResponse>, AppError> {
    let mut con = state.client.get_multiplexed_async_connection().await?;
    redis::cmd("FLUSHDB").query_async::<_, ()>(&mut con).await?;

    Ok(Json(MessageResponse {
        message: "Cleared DB Successful".to_string(),
    }))
}
