use encrypted_leaderboard::{app, state::AppState};

#[tokio::main]
async fn main() {
<<<<<<< HEAD
    let app = Router::new().merge(health::router(env!("CARGO_PKG_VERSION")));
=======
    let router = app(AppState::new(), env!("CARGO_PKG_VERSION"));
>>>>>>> main

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Leaderboard service on http://{addr}");
    axum::serve(listener, router).await.unwrap();
}
