use axum::{http::StatusCode, Json};
use aide::axum::{ApiRouter, routing::{get_with, post_with}};
use axum::extract::DefaultBodyLimit;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use tower_http::cors::{Any, CorsLayer};

#[derive(Serialize,Deserialize,JsonSchema)]
struct TestResponse {
    message : String,
}

pub(crate) async fn testfun() -> Result<Json<TestResponse>, (StatusCode, String)> {
    Ok(Json(TestResponse{
        message: "Success".to_string(),
    }))
}

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_router = ApiRouter::new()
    .api_route(
        "/", 
        post_with(testfun, |op| { 
            op.description("")
            .response::<200, Json<TestResponse>>() 
    }))
    .api_route(
        "/test",
        get_with(testfun, |op| {
            op.description("")
            .response::<200, Json<TestResponse>>()
        })
    );
    
    let app = openapi_docs::attach(
        api_router,
        "",
        "",
        ""
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
    .layer(cors);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
