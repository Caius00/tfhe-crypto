mod auction;
#[cfg(test)]
#[path = "tests/auktion_tests.rs"]
mod auktion_tests;

use aide::axum::ApiRouter;
use axum::extract::DefaultBodyLimit;
use tower_http::cors::{Any, CorsLayer};

pub fn api_router() -> aide::axum::ApiRouter {
    aide::axum::ApiRouter::new()
        .api_route(
            "/hallo",
            aide::axum::routing::get_with(auction::hallo_test, |op| {
                op.description("Test-Endpoint für den Auktions-Service.")
            }),
        )
        .api_route(
            "/gebot",
            aide::axum::routing::post_with(auction::gebot_empfangen, |op| {
                op.description("Empfängt und speichert ein FHE-verschlüsseltes Gebot.")
            }),
        )
        .api_route(
            "/auswerten",
            aide::axum::routing::get_with(auction::auktion_auswerten, |op| {
                op.description("Wertete die Auktion homomorph im verschlüsselten Zustand aus.")
            }),
        )
}

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = openapi_docs::attach(
        api_router(),
        "04 - Sealed-Bid Auction",
        "Homomorphic sealed-bid auction service: Submitting encrypted bids and \
         evaluating the winner server-side without revealing the actual amounts.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // Für die großen TFHE-Keys
    .layer(cors);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Fehler beim Binden an Port 8080: {}", e);
            std::process::exit(1);
        }
    };

    println!("\n🚀 Auktions-Backend läuft lokal auf http://127.0.0.1:8080");

    axum::serve(listener, app).await.unwrap();
}
