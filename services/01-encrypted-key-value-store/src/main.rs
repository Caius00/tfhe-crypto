//! Binary für den `encrypted-key-value-store`-Service.
//!
//! Hier wird der Prozess hochgefahren — Router-Aufbau und Konfiguration der
//! Layer (OpenAPI, Health, Metrics, Tracing, Body-Limit). Die eigentliche
//! Handler-Logik lebt in `routes.rs`.

use aide::axum::routing::post_with;
use aide::axum::ApiRouter;
use axum::extract::DefaultBodyLimit;
use encrypted_key_value_store::routes::{
    clear_entries, create_session, entry_exists, get_entry, put_entry,
};
use encrypted_key_value_store::store::{AppState, SharedState};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // OTLP-Tracing + strukturiertes Logging initialisieren — selber Pfad wie
    // in den anderen Services (siehe `services/08-encrypted-leaderboard/src/main.rs`).
    observability::init("encrypted-key-value-store", env!("CARGO_PKG_VERSION"));

    let state: SharedState = Arc::new(AppState::new());

    // Startup-Check: ist Redis wirklich erreichbar? Das ist die häufigste
    // Fehlerquelle (falscher Hostname, fehlendes Passwort, Pod-Restart ohne
    // Env). Wir loggen das Resultat klar und beenden den Prozess hart, wenn
    // die Verbindung fehlt — k8s startet den Pod sauber neu, und lokal sieht
    // man sofort, was los ist, statt jeden späteren Request scheitern zu sehen.
    match state.ping_redis().await {
        Ok(()) => tracing::info!(
            redis_endpoint = %state.redis_endpoint,
            "redis connection ok"
        ),
        Err(e) => {
            tracing::error!(
                redis_endpoint = %state.redis_endpoint,
                error = %e,
                "redis connection failed — refusing to start. \
                 Check REDIS_URL or REDIS_HOST/PORT/PASSWORD env vars."
            );
            observability::shutdown();
            std::process::exit(1);
        }
    }

    // Fachliche Routen — Pfade sind RELATIV zum Service-Root, weil Traefik
    // (Cluster) und Angular-Proxy (Dev) den Prefix `/kv` vorher strippen.
    let api_router = ApiRouter::new()
        .api_route(
            "/session",
            post_with(create_session, |op| {
                op.description(
                    "Open a new session: upload a bincode-serialized \
                     `CompressedServerKey` (base64). Returns a session_id.",
                )
            }),
        )
        .api_route(
            "/entry",
            post_with(put_entry, |op| {
                op.description(
                    "Store an encrypted (key, value) pair under the given session. \
                     Each chunk is one base64-encoded bincode-`FheUint8`.",
                )
            }),
        )
        .api_route(
            "/entry/get",
            post_with(get_entry, |op| {
                op.description(
                    "Fetch the encrypted value whose encrypted key matches — \
                     the server never learns which entry was selected.",
                )
            }),
        )
        .api_route(
            "/entry/exists",
            post_with(entry_exists, |op| {
                op.description(
                    "Return an encrypted FheBool indicating whether the key exists \
                     in this session.",
                )
            }),
        )
        .api_route(
            "/clear",
            post_with(clear_entries, |op| {
                op.description("Delete all entries of this session. Other sessions are untouched.")
            }),
        )
        .with_state(state);

    // Metrics-Layer + Prometheus-Route aus dem shared crate.
    let (metrics_layer, metrics_router) = metrics_exporter::setup();

    let app = openapi_docs::attach(
        api_router,
        "Encrypted Key-Value Store",
        "Homomorphic key-value store backed by Redis. Clients open a session by \
         uploading a CompressedServerKey, then put/get/exists/clear entries whose \
         keys and values are FHE-encrypted ASCII strings. Entries expire after \
         the configured TTL.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .merge(metrics_router)
    // ServerKeys können ~100 MB groß sein — Default-Body-Limit von axum würde sie cappen.
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
    .layer(metrics_layer)
    .layer(observability::http_trace_layer());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind TCP listener");
    tracing::info!(%addr, "encrypted-key-value-store listening");
    axum::serve(listener, app)
        .await
        .expect("axum serve terminated unexpectedly");

    observability::shutdown();
}
