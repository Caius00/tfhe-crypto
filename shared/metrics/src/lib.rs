use std::sync::OnceLock;

use axum::{routing::get, Router};
use axum_prometheus::{
    metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle},
    PrometheusMetricLayer, AXUM_HTTP_REQUESTS_DURATION_SECONDS,
};

// Der Recorder ist global – `install_recorder()` darf nur einmal pro
// Prozess laufen, sonst panic ("global recorder already set"). Tests
// rufen `app()` mehrfach auf, deshalb cachen wir den Handle in einem
// OnceLock.
static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

// Baut den Prometheus-Layer + /metrics-Route auf und liefert beides als
// chainable Router zurück. Aufrufer mergt den Router in seinen App-Router
// und legt den Layer als Middleware drauf:
//
//   let (metrics_layer, metrics_router) = metrics_exporter::setup();
//   app.merge(metrics_router).layer(metrics_layer)
//
// Histogramm-Buckets sind für API-Latenzen im Millisekunden-bis-Sekunden-Bereich
// ausgelegt – passt zu unseren FHE-Operationen, die je nach Endpoint
// zwischen 1ms und mehreren Sekunden liegen können.
pub fn setup() -> (PrometheusMetricLayer<'static>, Router) {
    let handle = METRICS_HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .set_buckets_for_metric(
                Matcher::Full(AXUM_HTTP_REQUESTS_DURATION_SECONDS.to_string()),
                &[
                    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
                ],
            )
            .expect("failed to set histogram buckets")
            .install_recorder()
            .expect("failed to install prometheus recorder")
    });
    let handle = handle.clone();

    let layer = PrometheusMetricLayer::new();
    let router = Router::new().route("/metrics", get(move || async move { handle.render() }));

    (layer, router)
}
