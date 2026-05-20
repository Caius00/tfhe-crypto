mod everything;

use axum::Router;

#[tokio::main]
async fn main() {
    // image_processing::convert_image_to_grayscale().expect("TODO: panic message");
    everything::image_processing().expect("TODO: panic message");

    // let app = Router::new().merge(health::router(env!("CARGO_PKG_VERSION")));
    //
    // let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    // let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    // axum::serve(listener, app).await.unwrap();
}
