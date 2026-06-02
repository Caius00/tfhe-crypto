pub mod functions;
#[cfg(test)]
mod auktion_tests;

use aide::axum::{routing::{get_with, post_with}, ApiRouter};
use axum::extract::DefaultBodyLimit;
use tower_http::cors::{Any, CorsLayer};


#[tokio::main]
async fn main() {

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    
    let api_router = ApiRouter::new()
        .api_route(
            "/test",
            get_with(functions::hallo_test, |op| {
                op.description("")
            }),
        )
        .api_route(
            "/gebot",
            post_with(functions::gebot_empfangen, |op| {
                op.description("")
            }),
        )
        .api_route(
            "/auswerten",
            post_with(functions::auktion_auswerten, |op| {
                op.description("")
            }),
        );

    let app = openapi_docs::attach(api_router, "sealed-bid-auction", "0.1", "")
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
        .layer(cors);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Fehler beim Binden an Port 8080: {}", e);
            std::process::exit(1);
        }
    };

    println!("Auktions-Server läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

