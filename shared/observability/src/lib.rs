use std::time::Duration;

use axum::http::Request;
use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    propagation::TraceContextPropagator,
    trace::{Sampler, TracerProvider},
    Resource,
};
use opentelemetry_semantic_conventions::{
    resource::{SERVICE_NAME, SERVICE_VERSION},
    SCHEMA_URL,
};
use tower_http::trace::{MakeSpan, TraceLayer};
use tracing::Span;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// OTLP gRPC Endpoint des Tempo-Singleton im Cluster.
// Override per Env `OTEL_EXPORTER_OTLP_ENDPOINT` (z.B. für lokales Dev).
const DEFAULT_OTLP_ENDPOINT: &str = "http://monitoring-tempo.monitoring.svc.cluster.local:4317";

// Initialisiert den globalen tracing-subscriber + OTLP-Exporter.
//
// Muss innerhalb eines Tokio-Runtimes aufgerufen werden (nicht im
// synchronen Teil von `main`), weil der OTLP-Tonic-Exporter eine async
// Channel-Pipeline aufbaut.
//
// Resource Attributes:
// - service.name        = `service_name`
// - service.version     = `version`
// - k8s.namespace.name  = Env $POD_NAMESPACE (per Downward API)
// - k8s.pod.name        = Env $POD_NAME (per Downward API)
//
// Weitere Attribute können über `OTEL_RESOURCE_ATTRIBUTES` ergänzt werden.
pub fn init(service_name: &'static str, version: &'static str) {
    global::set_text_map_propagator(TraceContextPropagator::new());

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());

    let mut attrs = vec![
        KeyValue::new(SERVICE_NAME, service_name),
        KeyValue::new(SERVICE_VERSION, version),
    ];
    if let Ok(ns) = std::env::var("POD_NAMESPACE") {
        attrs.push(KeyValue::new("k8s.namespace.name", ns));
    }
    if let Ok(pod) = std::env::var("POD_NAME") {
        attrs.push(KeyValue::new("k8s.pod.name", pod));
    }
    let resource = Resource::from_schema_url(attrs, SCHEMA_URL);

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(5))
        .build()
        .expect("failed to build OTLP exporter");

    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_sampler(Sampler::AlwaysOn)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer(service_name);
    global::set_tracer_provider(provider);

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let fmt_layer = tracing_subscriber::fmt::layer().with_target(true);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();
}

// Sauberes Flushen aller noch nicht exportierten Spans beim Shutdown.
// Sollte vor Programmende aufgerufen werden, sonst gehen die letzten
// Spans verloren (BatchExporter ist async).
pub fn shutdown() {
    global::shutdown_tracer_provider();
}

// Pfade die keine Traces erzeugen sollen: Kubernetes-Probes und der
// Prometheus-Scrape-Endpoint. Würden sonst ~10 Spans/Minute pro Pod
// produzieren (alle 10–15s) und Tempo mit Noise fluten.
const SKIP_TRACE_PATHS: &[&str] = &["/healthz", "/readyz", "/metrics"];

#[derive(Clone, Copy, Default)]
pub struct FilteredMakeSpan;

impl<B> MakeSpan<B> for FilteredMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        let path = request.uri().path();
        if SKIP_TRACE_PATHS.contains(&path) {
            // Span::none() ist "disabled" – kein OTLP-Export, kein Log-Output.
            Span::none()
        } else {
            tracing::info_span!(
                "http_request",
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version(),
            )
        }
    }
}

// Per-Request HTTP-Span Layer für Axum. Erzeugt für jeden Request einen
// Span (Method, URI, Status, Latenz) und propagiert einkommende
// `traceparent`-Header (Distributed Tracing). Health-Probes und
// `/metrics` werden ausgefiltert.
pub fn http_trace_layer() -> TraceLayer<
    tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>,
    FilteredMakeSpan,
> {
    TraceLayer::new_for_http().make_span_with(FilteredMakeSpan)
}
