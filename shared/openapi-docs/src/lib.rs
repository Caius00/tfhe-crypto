//! Hängt OpenAPI-Doku-Routen an einen `ApiRouter` an und finalisiert ihn.
//!
//! Liefert zwei Endpunkte:
//! - `GET /docs`         – Swagger UI
//! - `GET /openapi.json` – Raw-Spec (OpenAPI 3.1)
//!
//! Die Spec-URL ist relativ (`openapi.json`), damit das UI sowohl direkt am
//! Service-Port als auch hinter PathPrefix-Stripping (Traefik, Angular-Proxy)
//! funktioniert.

use std::sync::Arc;

use aide::{
    axum::{ApiRouter, IntoApiResponse},
    openapi::{Info, OpenApi},
    swagger::Swagger,
};
use axum::{http::header, routing::get, Extension, Router};

/// Hängt `/docs` + `/openapi.json` an den Router und gibt einen fertigen
/// `axum::Router` zurück. Das Spec wird einmal beim Startup serialisiert.
///
/// 
/// Aufrufer ist dafür verantwortlich, `with_state(...)` vorher zu setzen.
pub fn attach(router: ApiRouter, title: &str, description: &str, version: &str) -> Router {
    let mut api = OpenApi {
        info: Info {
            title: title.into(),
            description: Some(description.into()),
            version: version.into(),
            ..Info::default()
        },
        ..OpenApi::default()
    };

    let router = router
        .route("/docs", Swagger::new("openapi.json").axum_route())
        .route("/openapi.json", get(serve_openapi))
        .finish_api(&mut api);

    let spec_json = Arc::new(serde_json::to_string(&api).expect("serialize OpenAPI"));

    router.layer(Extension(spec_json))
}

async fn serve_openapi(Extension(spec): Extension<Arc<String>>) -> impl IntoApiResponse {
    (
        [(header::CONTENT_TYPE, "application/json")],
        spec.to_string(),
    )
}
