//! Encrypted Leaderboard — Crate-Wurzel.
//!
//! Architektur-Überblick:
//! - `state`   : Shared State (Sessions, Janitor) — `AppState`, `Session`, `EncEntry`
//! - `fhe`     : FHE-Engine pro Session (dekomprimierter ServerKey + rayon-Pool)
//! - `codec`   : Base64-Helfer für den HTTP-Transport von Ciphertexts
//! - `handlers`: Axum-Handler für die 5 öffentlichen Endpunkte
//!
//! Threat-Model in Kurzform:
//! - Der Service sieht **nur Ciphertexts**, nie Klartext-Scores.
//! - Sortierung passiert vollständig auf verschlüsselten Daten (FHE-Sortier-Netzwerk).
//! - Nur „E“ (der Raum-Ersteller) besitzt den ClientKey und kann Antworten entschlüsseln.

pub mod codec;
pub mod fhe;
pub mod handlers;
pub mod state;

use aide::axum::{
    routing::{get_with, post_with},
    ApiRouter,
};
use axum::{extract::DefaultBodyLimit, Router};

use crate::state::AppState;

/// Baut den vollständigen Axum-Router für den Leaderboard-Service.
///
/// Wird sowohl vom Binary (`main.rs`) als auch von Integrationstests
/// (`tests/api.rs`) aufgerufen — Tests übergeben dabei einen frischen
/// `AppState` und eine Dummy-Version.
///
/// Der Router enthält:
/// - 5 fachliche Endpunkte (siehe `api_route`-Aufrufe unten)
/// - `/healthz`, `/readyz`, `/version` aus `shared/health`
/// - `/metrics` (Prometheus) und distributed tracing layer aus `observability`
/// - `/docs` + `/openapi.json` (Swagger UI) aus `shared/openapi-docs`
pub fn app(state: AppState, version: &'static str) -> Router {
    // Prometheus-Exporter + Layer (zählt HTTP-Requests, Latenzen, etc.)
    let (metrics_layer, metrics_router) = metrics_exporter::setup();

    // Fachliche Routen — alle Pfade sind RELATIV zum Service-Root, weil
    // Traefik (Cluster) und Angular-Proxy (Dev) den Pfad-Prefix vorher strippen.
    let api_router = ApiRouter::new()
        .api_route(
            "/create",
            post_with(handlers::create_session, |op| {
                op.description("Create a new leaderboard room. Returns a 6-digit code.")
            }),
        )
        .api_route(
            "/{code}/public-key",
            get_with(handlers::get_public_key, |op| {
                op.description("Get the room's public key so players can encrypt scores.")
            }),
        )
        .api_route(
            "/{code}/submit",
            post_with(handlers::submit_score, |op| {
                op.description("Submit an encrypted score. Re-submits keep the FHE-max.")
            }),
        )
        .api_route(
            "/{code}/entries",
            get_with(handlers::get_entries, |op| {
                op.description("List entries — sorted view if ready, otherwise insertion order.")
            }),
        )
        .api_route(
            "/{code}/rank",
            post_with(handlers::query_rank, |op| {
                op.description("Rank lookup: per-position encrypted match bools.")
            }),
        )
        .with_state(state);

    // OpenAPI-Doku auf den ApiRouter ankleben, dann die übrigen Sub-Router mergen.
    // Reihenfolge der Layer am Ende: Body-Limit ZUERST (am äußeren Rand), damit
    // große ServerKey-Uploads (~hunderte MB) nicht von axum's Default (~2 MB) gekappt werden.
    openapi_docs::attach(
        api_router,
        "Encrypted Leaderboard",
        "Homomorphic leaderboard service: encrypted score submission, FHE-sorted \
         ranking, and encrypted rank queries — only E (the creator) can decrypt.",
        version,
    )
    .merge(health::router(version))
    .merge(metrics_router)
    // FHE-ServerKeys können bei /create > 100 MB groß sein — Default-Limit von axum reicht nicht.
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
    .layer(metrics_layer)
    .layer(observability::http_trace_layer())
}
