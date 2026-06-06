use std::sync::Arc;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use rayon::{ThreadPool, ThreadPoolBuilder};
use serde::{Deserialize, Serialize};
use tfhe::{set_server_key, CompressedServerKey, FheUint8, ServerKey};
use tokio::sync::Mutex;
use tokio::task::spawn_blocking;
use crate::encrypted_image::EncryptedImage;

pub struct SessionData {
    pub(crate) image: EncryptedImage,
    pub(crate) server_key: ServerKey, // TODO() remove server_key
    pub(crate) rayon_pool: ThreadPool,
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) current_session: Arc<Mutex<Option<SessionData>>>,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
pub struct ApiResponse {
    pub(crate) success: bool,
    pub(crate) message: String,
}

#[derive(Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub(crate) compressed_server_key: Vec<u8>,
    pub(crate) image_data: Vec<u8>, //TODO() compress image data
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Serialize, Deserialize)]
pub struct DeleteSessionResponse {
    pub(crate) image_data: Vec<u8>, //TODO() compress image data
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[axum::debug_handler]
pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let mut session_lock = state.current_session.lock().await;
    if session_lock.is_some() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse {
                success: false,
                message: "Another session is already active.".into(),
            }),
        );
    }

    let (compressed_server_key, image_data) = match (
        bincode::deserialize::<CompressedServerKey>(&body.compressed_server_key),
        bincode::deserialize::<Vec<FheUint8>>(&body.image_data)
        ) {
        (Ok(key), Ok(data)) => (key, data),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse {
                    success: false,
                    message: "Invalid request body.".into(),
                }),
            );
        }
    };

    let image = EncryptedImage {
        pixels: image_data,
        width: body.width,
        height: body.height,
    };

    let server_key = compressed_server_key.decompress();

    let pool_key = server_key.clone();
    let rayon_pool = ThreadPoolBuilder::new()
        .start_handler(move |_| {
            set_server_key(pool_key.clone());
        })
        .build()
        .unwrap();

    *session_lock = Some(SessionData {
        image,
        server_key,
        rayon_pool,
    });

    (
        StatusCode::CREATED,
        Json(ApiResponse {
            success: true,
            message: "Session created successfully.".into(),
        }),
    )
}

pub async fn delete_session(
    State(state): State<AppState>,
) -> Result<Json<DeleteSessionResponse>, (StatusCode, Json<ApiResponse>)> {
    let session = {
        let mut lock = state.current_session.lock().await;

        match lock.take() {
            Some(session) => session,
            None => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse {
                        success: false,
                        message: "No active session to delete.".into(),
                    })
                ));
            }
        }
    };

    Ok(Json(DeleteSessionResponse {
        image_data: bincode::serialize(&session.image.pixels).unwrap(),
        width: session.image.width,
        height: session.image.height,
    }))
}

pub struct ImageOperation;

impl ImageOperation {

    async fn run_image_operation<F>(
        state: AppState,
        operation: F,
    ) -> (StatusCode, Json<ApiResponse>)
    where F: FnOnce(&mut EncryptedImage) + Send + 'static,{
        let mut session = {
            let mut lock = state.current_session.lock().await;

            match lock.take() {
                Some(session) => session,
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ApiResponse {
                            success: false,
                            message: "No active session.".into(),
                        }),
                    );
                }
            }
        };

        let session = spawn_blocking(move || {
            session.rayon_pool.install(|| {
                operation(&mut session.image);
            });

            session
        })
            .await
            .unwrap();

        {
            let mut lock = state.current_session.lock().await;
            *lock = Some(session);
        }
        (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: "Successfully manipulated image.".into(),
            }),
        )
    }

    // PER PIXEL
    pub async fn invert(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.invert()
        }).await
    }
    pub async fn white_threshold(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.white_threshold()
        }).await
    }
    pub async fn black_threshold(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.invert()
        }).await
    }

    // ROTATION
    pub async fn rotate_90(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.rotate_90()
        }).await
    }
    pub async fn rotate_180(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.rotate_180()
        }).await
    }
    pub async fn rotate_270(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.rotate_270()
        }).await
    }

    // FLIP
    pub async fn flip_vertical(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.flip_vertical()
        }).await
    }
    pub async fn flip_horizontal(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.flip_horizontal()
        }).await
    }

    // BLURRING AND BLOOMING
    pub async fn blur(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.box_blur()
        }).await
    }
    pub async fn bloom(
        State(state): State<AppState>,
    ) -> (StatusCode, Json<ApiResponse>) {
        Self::run_image_operation(state, |image| {
            image.blooming()
        }).await
    }
}