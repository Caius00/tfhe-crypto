use axum::{http::StatusCode, Json};
use aide::axum::{routing::post_with, ApiRouter};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

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
    let api_router = ApiRouter::new().api_route(
        "/", 
        post_with(testfun, |op| { 
            op.description("")
            .response::<200, Json<TestResponse>>() 
        }),
    );
    
    let app = openapi_docs::attach(
        api_router,
        "",
        "",
        ""
    ).merge(health::router(env!("CARGO_PKG_VERSION")));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
