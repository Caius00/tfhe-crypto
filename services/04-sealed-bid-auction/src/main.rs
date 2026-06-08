mod auction;
#[cfg(test)]
#[path = "tests/auktion_tests.rs"]
mod auktion_tests;

use aide::axum::{
    routing::{get_with, post_with},
    ApiRouter,
};
use axum::{extract::DefaultBodyLimit, Router};
use tower_http::cors::{Any, CorsLayer};

pub fn create_app() -> Router {
    // Hier hängen die Routen OHNE /api (genau wie bei deinem Kommilitonen)
    let api_router = ApiRouter::new()
        .api_route(
            "/gebot",
            post_with(auction::gebot_empfangen, |op| {
                op.description("Empfängt ein verschlüsseltes Gebot.")
            }),
        )
        .api_route(
            "/auswerten",
            get_with(auction::auktion_auswerten, |op| {
                op.description("Wertet die Auktion homomorph aus.")
            }),
        );

    openapi_docs::attach(
        api_router,
        "Sealed-Bid Auction Service",
        "Homomorphic sealed-bid auction service.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
}

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = create_app().layer(cors);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("🚀 Server läuft auf http://{}", addr);

    axum::serve(listener, app).await.unwrap();
}
