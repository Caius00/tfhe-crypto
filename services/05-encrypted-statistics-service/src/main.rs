#[tokio::main]
async fn main() {
    tokio::spawn(health::serve(8080, env!("CARGO_PKG_VERSION")));
    tokio::signal::ctrl_c().await.unwrap();
}
