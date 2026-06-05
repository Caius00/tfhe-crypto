mod everything;
mod routes;
mod encrypted_image;

use std::sync::Arc;
use axum::{serve, Router};
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use tokio::sync::{Mutex};
use crate::routes::{create_session, delete_session, AppState, ImageOperation};

#[tokio::main]
async fn main() {
    let state = AppState {
        current_session: Arc::new(Mutex::new(None)),
    };

    let app = Router::new()
        .route("/session", post(create_session))
        .route("/session", delete(delete_session))
        .route("/per-pixel/invert", post(ImageOperation::invert))
        .route("/per-pixel/white-threshold", post(ImageOperation::white_threshold))
        .route("/per-pixel/black-threshold", post(ImageOperation::black_threshold))
        // rotate
        // flip
        .with_state(state)
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
        .merge(health::router(env!("CARGO_PKG_VERSION")));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use std::time::Instant;
    use axum::body::Bytes;
    use axum::Router;
    use axum_test::http::StatusCode;
    use axum_test::TestServer;
    use image::{DynamicImage, ImageReader};
    use rayon::prelude::*;
    use serde_json::json;
    use tfhe::{ClientKey, CompressedServerKey, FheUint8};
    use tfhe::prelude::FheEncrypt;
    use tfhe::shortint::parameters::{Log2PFail, MetaParametersFinder};
    use tfhe::shortint::parameters::Backend::Cpu;
    use tfhe::shortint::parameters::Constraint::LessThanOrEqual;
    use crate::encrypted_image::EncryptedImage;
    use crate::routes::{ApiResponse, CreateSessionRequest};
    use super::*;

    fn build_test_server() -> TestServer {
        let state = AppState {
            current_session: Arc::new(Mutex::new(None)),
        };

        let app = Router::new()
            .route("/session", post(create_session))
            .route("/session", delete(delete_session))
            .route("/per-pixel/invert", post(ImageOperation::invert))
            .route("/per-pixel/white-threshold", post(ImageOperation::white_threshold))
            .route("/per-pixel/black-threshold", post(ImageOperation::black_threshold))
            // rotate
            // flip
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
            .with_state(state);

        TestServer::new(app)
    }

    fn preprocess_image(
        clean_img: DynamicImage,
        client_key: &ClientKey
    ) -> EncryptedImage {
        /// returns encrypted image data and image dimensions width, height

        let width = clean_img.width() as usize;
        let height = clean_img.height() as usize;

        // maybe use into_luma8 to convert to grayscale instead?
        let raw_image_data = clean_img.to_luma8().into_raw();

        let now = Instant::now();

        let pixels: Vec<FheUint8> = raw_image_data
            .par_iter()
            .map(
                |&value| FheUint8::encrypt(value, client_key),
            ).collect();

        let elapsed = now.elapsed();
        println!("Encrypt Finished in: {:?}", elapsed);

        EncryptedImage {
            pixels,
            width,
            height,
        }
    }

    async fn create_test_session() -> ApiResponse {
        let server = build_test_server();

        let clean_img = ImageReader::open("resources/images/original.png")
            .unwrap()
            .decode()
            .unwrap();

        let parameters = MetaParametersFinder::new(
            LessThanOrEqual(Log2PFail(-128.0)),
            Cpu
        )
            .with_compression(true)
            .find()
            .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);

        let compressed_server_key = CompressedServerKey::new(&client_key);
        let encrypted_image = preprocess_image(clean_img, &client_key);

        let request_body = json!(CreateSessionRequest{
            compressed_server_key: bincode::serialize(&compressed_server_key).unwrap(),
            image_data: bincode::serialize(&encrypted_image.pixels).unwrap(),
            width: encrypted_image.width,
            height: encrypted_image.height
        });

        let response = server
            .post("/session")
            .json(&request_body)
            .await;

        response.assert_status(StatusCode::CREATED);
        let body: ApiResponse = response.json();

        body
    }
    #[tokio::test]
    async fn test_create_session_happy_path() {
        let body = create_test_session().await;
        assert_eq!(body.success, true);
        assert_eq!(body.message, "Session created successfully.");
    }

    #[tokio::test]
    async fn test_invert() {
        let server = build_test_server();
        create_test_session().await;

        let response = server.post("/per-pixel/invert").await;

        response.assert_status(StatusCode::OK);
        response.assert_json(&ApiResponse {
            success: true,
            message: "Successfully manipulated image.".to_string(),
        });
    }

    #[tokio::test]
    async fn test_delete_session_happy_path() {
        let server = build_test_server();
        let fake_image = vec![0, 1, 2, 3];

        // Setup: Create session first
        let setup = server
            .post("/session")
            .add_header("x-server-key", "delete_key")
            .bytes(Bytes::from(fake_image))
            .await;
        setup.assert_status(StatusCode::CREATED);

        // Actual Test
        let response = server.delete("/session").await;

        response.assert_status(StatusCode::OK);
        response.assert_json(&ApiResponse {
            success: true,
            message: "Session deleted. Server is now available.".to_string(),
        });
    }
}
