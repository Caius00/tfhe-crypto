//! Binary für den `encrypted-leaderboard`-Service.
//!
//! Hier wird nur der Prozess hochgefahren — die eigentliche Router-Logik
//! lebt in `lib.rs`, damit Integrationstests denselben Aufbau wiederverwenden
//! können (siehe `tests/api.rs`).

use encrypted_leaderboard::{
    app,
    state::{AppState, JANITOR_INTERVAL, SESSION_IDLE_TIMEOUT},
};

#[tokio::main]
async fn main() {
    // OTLP-Tracing + strukturiertes Logging initialisieren (shared crate `observability`)
    observability::init("encrypted-leaderboard", env!("CARGO_PKG_VERSION"));

    // Shared State: HashMap<Raumcode, Arc<Session>>, geteilt zwischen allen Handlern.
    let state = AppState::new();

    // Hintergrund-Janitor entfernt Sessions, an denen 10 Minuten lang nichts passiert ist.
    // Wichtig, weil jede Session ~100–500 MB dekomprimierten ServerKey hält — ohne
    // Eviction OOM-Risiko sobald mehrere Räume hintereinander erstellt werden.
    state.spawn_janitor(SESSION_IDLE_TIMEOUT, JANITOR_INTERVAL);

    // Router mit allen API-Routen + /healthz + /metrics + /docs zusammenbauen.
    let router = app(state, env!("CARGO_PKG_VERSION"));

    // Bind auf 0.0.0.0:8080 — der Service kennt seinen externen Pfad NICHT.
    // Traefik (im k8s-Cluster) bzw. der Angular-Dev-Proxy strippt den Pfad-Prefix.
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!(%addr, "leaderboard service listening");
    axum::serve(listener, router).await.unwrap();

    // Tracing-Provider sauber herunterfahren (flusht offene Spans/Metriken).
    observability::shutdown();
}
