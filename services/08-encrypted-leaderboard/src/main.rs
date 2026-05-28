use encrypted_leaderboard::{app, state::AppState};

#[tokio::main]
async fn main() {
    observability::init("encrypted-leaderboard", env!("CARGO_PKG_VERSION"));

    let router = app(AppState::new(), env!("CARGO_PKG_VERSION"));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!(%addr, "leaderboard service listening");
    axum::serve(listener, router).await.unwrap();

    observability::shutdown();
}
