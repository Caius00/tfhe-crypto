# Infrastructure

Cluster-Basiskomponenten. Reihenfolge beim erstmaligen Aufsetzen:

1. Gateway API CRDs (siehe unten)
2. `traefik/` – API Gateway / LoadBalancer
3. `argocd/` – GitOps-Sync für alle weiteren Services
4. `monitoring/` – Prometheus + Grafana + Alertmanager + Tempo

## Gateway API CRDs (Bootstrap)

Das Cluster benötigt die **Experimental Channel** der Gateway API CRDs, weil
neben `HTTPRoute` (Standard) auch `TCPRoute` genutzt wird (z. B. um Redis von
Service 1 öffentlich via Gateway erreichbar zu machen).

Einmalig pro Cluster anwenden:

```bash
kubectl apply -f https://github.com/kubernetes-sigs/gateway-api/releases/download/v1.2.0/experimental-install.yaml
```

Das Manifest enthält sowohl Standard- als auch Experimental-CRDs – ein
zusätzlicher `standard-install` ist nicht nötig.

## Traefik

Helm-Wrapper um den offiziellen Traefik-Chart. Stellt das `traefik-gateway`
(Gateway API) bereit mit folgenden Listenern:

| Listener | Protocol | Port | Zweck                                   |
|----------|----------|------|------------------------------------------|
| `web`    | HTTP     | 8000 | HTTPRoutes für alle Services             |
| `redis`  | TCP      | 6379 | TCPRoute für Redis aus Service 1         |

Damit der TCP-Listener funktioniert, ist im Chart
`providers.kubernetesGateway.experimentalChannel: true` gesetzt – sonst ignoriert
Traefik `TCPRoute`/`TLSRoute`-Ressourcen.

Der LoadBalancer-Service exponiert beide Ports nach außen. Redis ist nach
Deploy erreichbar über:

```bash
redis-cli -h <LoadBalancer-IP> -p 6301 -a <password>
```

Hinweis: Redis ist damit ohne TLS öffentlich, nur durch Passwort geschützt –
für produktive Nutzung ggf. auf TLSRoute (Passthrough) oder Bastion umstellen.

## ArgoCD

GitOps-Controller, synct die Charts unter `helm/services/` automatisch auf den
Cluster.

## Monitoring (Prometheus / Grafana / Alertmanager / Tempo)

Bundle-Chart, das `kube-prometheus-stack` und `grafana/tempo` als
Dependencies zusammenfasst. Installation:

```bash
helm dependency update helm/infrastructure/monitoring
helm install monitoring helm/infrastructure/monitoring -n monitoring --create-namespace
```

**Prometheus scrape-Verhalten:**
- `*SelectorNilUsesHelmValues: false` → Prometheus pickt ServiceMonitors,
  PodMonitors, Rules und Probes aus **allen** Namespaces, unabhängig vom
  Helm-Release-Label.
- Zusätzlicher `kubernetes-pods` Job: jeder Pod mit Annotation
  `prometheus.io/scrape: "true"` (optional `prometheus.io/port` und
  `prometheus.io/path`) wird automatisch gescraped, auch ohne ServiceMonitor.

**Grafana:**
- Erreichbar unter `http://159.195.145.100/grafana/` (Login: `admin`/`admin`)
- Tempo ist bereits als Datasource verdrahtet → Trace-Suche und Service-Graph
  direkt aus den Default-Dashboards heraus nutzbar.
- ConfigMaps mit Label `grafana_dashboard: "1"` werden aus allen Namespaces
  als zusätzliche Dashboards importiert.

**Tempo:**
- Single-Binary StatefulSet, lokales Storage-Backend (10Gi).
- Trace-Ingest-Endpoints (im Cluster):
  - OTLP gRPC: `monitoring-tempo.monitoring:4317`
  - OTLP HTTP: `monitoring-tempo.monitoring:4318`
  - Jaeger HTTP: `monitoring-tempo.monitoring:14268`
  - Zipkin: `monitoring-tempo.monitoring:9411`

**k3s-Hinweis:** ServiceMonitors für `kube-controller-manager`,
`kube-scheduler`, `kube-proxy` und `kube-etcd` sind deaktiviert – diese
Komponenten laufen bei k3s im Hauptbinary und exponieren keine eigenen
Endpoints.
