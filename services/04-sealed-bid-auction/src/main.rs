#[tokio::main]
async fn main() {
    tokio::spawn(health::serve(8080));
    println!("Hello from Sealed-Bid Auction!");
    tokio::signal::ctrl_c().await.unwrap();
}
