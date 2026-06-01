# Observability in einem Service einbauen

So bekommst du Metriken (Prometheus) und Traces (Tempo) für einen neuen
oder bestehenden Service. Beispiel-Referenz: `services/08-encrypted-leaderboard`.

Codeblöcke nutzen `diff`-Format — `+` Zeilen sind neu hinzugefügt.

## 1. Cargo-Dependencies

`services/<dein-service>/Cargo.toml`:

```diff
 [dependencies]
 axum.workspace = true
 health.workspace = true
+metrics-exporter.workspace = true
+observability.workspace = true
 openapi-docs.workspace = true
+tracing = "0.1"
```

## 2. `main.rs` — Init + Shutdown

```diff
 #[tokio::main]
 async fn main() {
+    observability::init("dein-service", env!("CARGO_PKG_VERSION"));
+
     let router = app(AppState::new(), env!("CARGO_PKG_VERSION"));
     let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
     let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
+    tracing::info!(%addr, "service listening");
     axum::serve(listener, router).await.unwrap();
+
+    observability::shutdown();
 }
```

## 3. `lib.rs` — Layer in den Router hängen

```diff
 pub fn app(state: AppState, version: &'static str) -> Router {
+    let (metrics_layer, metrics_router) = metrics_exporter::setup();
+
     let api_router = ApiRouter::new()
         .api_route(...)
         .with_state(state);

     openapi_docs::attach(api_router, "...", "...", version)
         .merge(health::router(version))
+        .merge(metrics_router)                       // /metrics Endpoint
         .layer(DefaultBodyLimit::max(...))
+        .layer(metrics_layer)                        // Request-Counter + Latenz
+        .layer(observability::http_trace_layer())    // Ein Span pro HTTP-Request
 }
```

## 4. Eigene Spans für teure Operationen

Pro Funktion, die du als eigenen Trace-Abschnitt sehen willst, ein Attribut drauf:

```diff
+#[tracing::instrument(skip(self, pairs), fields(n = pairs.len()))]
 pub fn sort_by_score_desc(&self, pairs: &mut [...]) -> Result<(), String> {
     ...
 }
```

- `skip(...)` lässt große Argumente aus dem Span weg (Default loggt sie als String).
- `fields(...)` legt zusätzliche Attribute drauf — z.B. die Anzahl Items.

Innerhalb von `tokio::spawn(async move { ... })` musst du `.instrument(span)`
statt `#[instrument]` nutzen, weil der Span sonst nicht in die spawned Task wandert:

```diff
 fn spawn_sort_if_idle(session: Arc<Session>) {
+    use tracing::Instrument;
+    let span = tracing::info_span!("background_sort");
     tokio::spawn(async move {
         ...
-    });
+    }.instrument(span));
 }
```

## 5. Helm-Chart — Pod-Annotations + Env

`helm/services/<dein-service>/templates/deployment.yaml`:

```diff
 template:
   metadata:
     labels:
       app: {{ .Chart.Name }}
+    annotations:
+      prometheus.io/scrape: "true"
+      prometheus.io/port: "{{ .Values.service.port }}"
+      prometheus.io/path: "/metrics"
   spec:
     containers:
       - name: {{ .Chart.Name }}
         image: "..."
+        env:
+          - name: POD_NAME
+            valueFrom: { fieldRef: { fieldPath: metadata.name } }
+          - name: POD_NAMESPACE
+            valueFrom: { fieldRef: { fieldPath: metadata.namespace } }
+          - name: OTEL_EXPORTER_OTLP_ENDPOINT
+            value: "http://monitoring-tempo.monitoring.svc.cluster.local:4317"
+          - name: RUST_LOG
+            value: "info"
         ports:
           - containerPort: {{ .Values.service.port }}
```

Die Annotationen reichen Prometheus, weil der `kubernetes-pods`-Scrape-Job
in `helm/infrastructure/monitoring/values.yaml` Pods mit `prometheus.io/scrape`
automatisch aufnimmt.

## 6. Verifizieren

```bash
# Metriken erreichbar?
kubectl exec -n <ns> deploy/<service> -- wget -qO- localhost:8080/metrics | head

# Im Grafana-Dashboard "Service Overview" deinen Namespace auswählen.
# Traces in Grafana → Explore → Datasource "Tempo" →
#   TraceQL: { resource.k8s.namespace.name="<ns>" }
```

## Was bekommst du out of the box

- **Metriken:** `axum_http_requests_total{status, method, endpoint}`,
  `axum_http_requests_duration_seconds_bucket` (Histogram für p50/p95/p99),
  `axum_http_requests_pending`.
- **Traces:** ein Span pro HTTP-Request (Method, Pfad, Status, Latenz) +
  jede `#[instrument]`-annotierte Funktion als Child-Span darunter.
- **Logs:** strukturierte Logs (`tracing::info!`, `warn!`, `error!`) gehen
  auf stdout — werden per `RUST_LOG` gefiltert.
