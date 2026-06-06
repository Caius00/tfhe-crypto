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
        .route("/rotate/90", post(ImageOperation::rotate_90))
        .route("/rotate/180", post(ImageOperation::rotate_180))
        .route("/rotate/270", post(ImageOperation::rotate_270))
        .route("/flip/vertical", post(ImageOperation::flip_vertical))
        .route("/flip/horizontal", post(ImageOperation::flip_horizontal))
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
    use axum::Router;
    use axum_test::http::StatusCode;
    use axum_test::TestServer;
    use image::{DynamicImage, GrayImage, ImageBuffer, ImageReader, Luma};
    use rayon::prelude::*;
    use serde_json::json;
    use tfhe::{ClientKey, CompressedServerKey, FheUint8};
    use tfhe::prelude::{FheDecrypt, FheEncrypt};
    use tfhe::shortint::parameters::{Log2PFail, MetaParametersFinder};
    use tfhe::shortint::parameters::Backend::Cpu;
    use tfhe::shortint::parameters::Constraint::LessThanOrEqual;
    use crate::encrypted_image::EncryptedImage;
    use crate::routes::{ApiResponse, CreateSessionRequest, DeleteSessionResponse};
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
            .route("/rotate/90", post(ImageOperation::rotate_90))
            .route("/rotate/180", post(ImageOperation::rotate_180))
            .route("/rotate/270", post(ImageOperation::rotate_270))
            .route("/flip/vertical", post(ImageOperation::flip_vertical))
            .route("/flip/horizontal", post(ImageOperation::flip_horizontal))
            .route("/effects/blur", post(ImageOperation::blur))
            .route("/effects/bloom", post(ImageOperation::bloom))
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
            .with_state(state);

        TestServer::new(app)
    }

    fn preprocess_image(
        clean_img: DynamicImage,
        client_key: &ClientKey
    ) -> EncryptedImage {
        // returns encrypted image data and image dimensions width, height

        let width = clean_img.width();
        let height = clean_img.height();

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

    async fn create_test_session(server: &TestServer, client_key: &ClientKey) -> ApiResponse {
        let clean_img = ImageReader::open("resources/images/original.png")
            .unwrap()
            .decode()
            .unwrap();

        let compressed_server_key = CompressedServerKey::new(client_key);
        let encrypted_image = preprocess_image(clean_img, client_key);

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
        let server = build_test_server();
        let parameters = MetaParametersFinder::new(
            LessThanOrEqual(Log2PFail(-128.0)),
            Cpu
        )
            .with_compression(true)
            .find()
            .expect("Could not find suitable parameters");
        let client_key = ClientKey::generate(parameters);

        let body = create_test_session(&server, &client_key).await;
        assert_eq!(body.success, true);
        assert_eq!(body.message, "Session created successfully.");
        
        // TODO() check if only one session can exist
    }

    #[tokio::test]
    async fn test_invert() {
        let server = build_test_server();
        let parameters = MetaParametersFinder::new(
            LessThanOrEqual(Log2PFail(-128.0)),
            Cpu
        )
            .with_compression(true)
            .find()
            .expect("Could not find suitable parameters");
        let client_key = ClientKey::generate(parameters);

        create_test_session(&server, &client_key).await;

        let response = server.post("/per-pixel/invert").await;

        response.assert_status(StatusCode::OK);
        response.assert_json(&ApiResponse {
            success: true,
            message: "Successfully manipulated image.".to_string(),
        });
        
        // TODO() somehow verify all ops automated
    }

    fn postprocess_image(
        processed_enc_img_data: &Vec<FheUint8>,
        width: u32,
        height: u32,
        img_name: &str,
        client_key: &ClientKey,
    ) -> ImageBuffer<Luma<u8>, Vec<u8>> {
        let now = Instant::now();

        let plain_processed_img: Vec<u8> = processed_enc_img_data
            .par_iter()
            .map(
                |value| value.decrypt(&client_key),
            ).collect();
        let elapsed = now.elapsed();
        println!("Decrypt Finished in: {:?}", elapsed);

        let processed_img = GrayImage::from_raw(width, height, plain_processed_img).unwrap();

        let img_location = format!("resources/images/processed/{}.png", img_name);
        processed_img.save(img_location).expect("TODO: panic message");

        println!("Storing Image finished: {:?}", now.elapsed());
        processed_img
    }

    #[tokio::test]
    async fn test_delete_session_happy_path() {
        let server = build_test_server();
        let parameters = MetaParametersFinder::new(
            LessThanOrEqual(Log2PFail(-128.0)),
            Cpu
        )
            .with_compression(true)
            .find()
            .expect("Could not find suitable parameters");
        let client_key = ClientKey::generate(parameters);


        create_test_session(&server, &client_key).await;

        server.post("/effects/blur").await;

        let response = server.delete("/session").await;
        response.assert_status_ok();

        let body: DeleteSessionResponse = response.json();
        let img_data: Vec<FheUint8> = bincode::deserialize(&body.image_data).unwrap();

        postprocess_image(&img_data, body.width, body.height, "TestBlur", &client_key);
        
        // TODO() test if session is actually deleted
        // TODO() Dont write images to storage; verify authenticity in RAM
    }


    
    #[tokio::test]
    async fn test_blur() {
        let server = build_test_server();

        let parameters = MetaParametersFinder::new(
            LessThanOrEqual(Log2PFail(-128.0)),
            Cpu,
        )
            .with_compression(true)
            .find()
            .expect("Could not find suitable parameters");
        let client_key = ClientKey::generate(parameters);

        create_test_session(&server, &client_key).await;

        let response = server.post("/effects/blur").await;
        response.assert_status(StatusCode::OK);

        response.assert_json(&ApiResponse {
            success: true,
            message: "Successfully manipulated image.".to_string(),
        });

        let response = server.delete("/session").await;

        response.assert_status_ok();

        let body: DeleteSessionResponse = response.json();

        let img_data: Vec<FheUint8> =
            bincode::deserialize(&body.image_data)
                .unwrap();

        postprocess_image(
            &img_data,
            body.width,
            body.height,
            "BlurTest",
            &client_key,
        );
    }

    #[tokio::test]
    async fn test_blur_then_bloom() {
        let server = build_test_server();

        let parameters = MetaParametersFinder::new(
            LessThanOrEqual(Log2PFail(-128.0)),
            Cpu,
        )
        .with_compression(true)
        .find()
        .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);

        create_test_session(&server, &client_key).await;

        server.post("/effects/blur").await;
        server.post("/effects/bloom").await;

        let response = server.delete("/session").await;

        let body: DeleteSessionResponse = response.json();

        let img_data: Vec<FheUint8> =
            bincode::deserialize(&body.image_data)
                .unwrap();

        postprocess_image(
            &img_data,
            body.width,
            body.height,
            "BlurThenBloomTest",
            &client_key,
        );
    }
}
