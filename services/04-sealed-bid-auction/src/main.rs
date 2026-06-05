mod auction;
#[cfg(test)]
#[path = "tests/auktion_tests.rs"]
mod auktion_tests;

pub fn api_router() -> aide::axum::ApiRouter {
    aide::axum::ApiRouter::new()
        .api_route(
            "/hallo",
            aide::axum::routing::get_with(auction::hallo_test, |op| op),
        )
        .api_route(
            "/gebot",
            aide::axum::routing::post_with(auction::gebot_empfangen, |op| op),
        )
        .api_route(
            "/auswerten",
            aide::axum::routing::get_with(auction::auktion_auswerten, |op| op),
        )
}

#[tokio::main]
async fn main() {
    // strart server
}
