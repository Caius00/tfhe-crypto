#[tokio::main]
async fn main() {
    tokio::spawn(health::serve(8080, env!("CARGO_PKG_VERSION")));
    println!("Hello from Encrypted Statistics Service!");
    tokio::signal::ctrl_c().await.unwrap();
}
