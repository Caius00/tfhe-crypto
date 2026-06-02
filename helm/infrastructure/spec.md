# Infrastructure

## Verwendete Patterns

Die Infrastruktur folgt einigen Standard-Patterns aus dem Cloud-Native-Bereich, die
nicht eigens erfunden, sondern gezielt kombiniert wurden:

- **GitOps** - der Soll-Zustand des Clusters liegt im Git-Repo. ArgoCD vergleicht
  laufend und gleicht ab.
- **Gateway-Pattern** - alle externen Requests laufen über einen einzigen
  reverse-proxy-artigen Eintrittspunkt (Traefik), nicht direkt an einzelne Services.
- **Scale-to-Zero** - KEDA fährt Pods bei Inaktivität auf 0 und bei eingehendem
  Traffic wieder hoch. Spart Ressourcen ohne dass der Dienst „weg" ist.
- **Helm Umbrella Chart / Wrapper Chart** - wir schreiben keine Kubernetes-Manifeste
  von Grund auf, sondern wickeln die offiziellen Community-Charts (Traefik, ArgoCD,
  KEDA, kube-prometheus-stack, Tempo) als Dependencies in eigene „Wrapper"-Charts
  ein. So bleibt nur unsere Konfiguration im Repo, der Großteil kommt fertig aus
  öffentlichen Helm-Repositories.
- **Multi-Stage-Docker-Build** mit `cargo-chef` - siehe Build-Sektion unten.

## Helm - Paket-Manager für Kubernetes

**Helm** ist der Standard-Paket-Manager für Kubernetes (analog zu `apt` oder `npm`).
Ein Helm-Chart bündelt eine Sammlung von Kubernetes-Manifesten mit Platzhaltern
(Templates) und einer `values.yaml`, in der die Konfiguration konkret befüllt wird.
Im Repo:

```
helm/
├── infrastructure/         # Plattform-Komponenten (manuell installiert)
│   ├── traefik/            # Wrapper um traefik/traefik
│   ├── argocd/             # Wrapper um argo/argo-cd
│   ├── keda/               # Wrapper um kedacore/keda
│   ├── keda-http/          # Wrapper um kedacore/keda-add-ons-http
│   └── monitoring/         # Wrapper um kube-prometheus-stack + grafana/tempo
└── services/               # Pro Use Case ein Chart (von ArgoCD auto-synced)
    ├── 01-encrypted-key-value-store/
    ├── ...
    └── 09-encrypted-program-execution/
```

Jeder „Wrapper" enthält eine `Chart.yaml` mit den offiziellen Charts als Dependencies
und eine `values.yaml` mit projektspezifischen Anpassungen. Vor dem Install muss einmal
`helm dependency update` laufen, das holt die externen Charts als `.tgz` in den
`charts/`-Unterordner.

## Cluster

Alles läuft auf einem **Hetzner-Server** (AMD EPYC 9645, 8 Kerne, 16 GiB RAM,
IP `159.195.145.100`). Darauf ein einzelner Kubernetes-Knoten in der schlanken Variante
**k3s**. CI/CD läuft komplett über **GitHub-hosted Runner** (`ubuntu-latest`). Auf dem
Cluster selbst gibt es keine self-hosted Runner.

**Gateway API - Experimental Channel**: Die Gateway API ist die neue, vom Kubernetes-
Projekt offiziell empfohlene Nachfolge des klassischen `Ingress`. Sie wird in zwei
Channels veröffentlicht: *Standard* (stabil, nur HTTP) und *Experimental* (zusätzlich
TCP- und TLS-Routing, noch nicht final standardisiert). Wir verwenden den **Experimental
Channel in Version 1.2.0**, weil Service 1 (Redis) und Service 6 (Postgres) ihre
Datenbanken auch von außerhalb des Clusters erreichbar machen - das geht nur über
`TCPRoute`, das im Standard-Channel noch nicht enthalten ist.

## 4.2 Gateway und Routing

Im Cluster läuft nur **Traefik** als öffentliche Eintrittstür. Er nimmt HTTP auf Port 80
entgegen und entscheidet anhand des **Pfads**, an welchen Service der Request weitergeht
(z. B. `/leaderboard/...` --> Leaderboard-Service). Für die Datenbank-Zugriffe von außen
(Service 1 Redis, Service 6 Postgres) kommen TCP-Listener auf Port 6301 bzw. 5432 dazu.

**Was Traefik mit einem Request macht** (z. B. `GET /leaderboard/123/entries`):

1. Schaut auf den Pfad-Präfix - passt zur HTTPRoute des Leaderboard-Services.
2. Strippt den Präfix weg, der Service empfängt nur noch `/123/entries`. Dadurch wissen
   die Services nichts von ihrem öffentlichen Pfad - das macht sie umzieh-bar.
3. Setzt einen internen Host-Header, damit der nachgeschaltete KEDA-Interceptor (siehe
   unten) weiß, welcher Service gemeint ist.
4. Leitet den Request weiter - nicht direkt an den Service-Pod, sondern erst an den
   Interceptor.

**Was Traefik sieht**: HTTP-Methode, Pfad, Header, Body - den Body aber nur als Byte-Strom,
ohne ihn zu lesen oder zu verändern. **Keine Authentifizierung, kein Rate-Limit, keine
WAF** - jeder, der die URL kennt, kann beliebige Requests an jeden Service schicken.

Kurze Begriffsklärung dazu:

- **WAF** (Web Application Firewall) - Filter, der HTTP-Requests auf bekannte
  Angriffsmuster (SQL-Injection, XSS, Path-Traversal, …) prüft und blockiert.
- **OIDC** (OpenID Connect) - Standard für Authentifizierung über einen externen
  Identity-Provider (Google, GitHub, Keycloak, …). Der Nutzer loggt sich beim Provider
  ein, der stellt ein signiertes Token aus, das die Anwendung dann prüft.

Das ist im Rahmen eines Uni-Projekts bewusst so akzeptiert: das System ist kein
Produkt, sondern eine Lernumgebung für FHE. Für einen Produktiv-Betrieb müsste ein
Auth-Layer (OIDC am Gateway oder Token-Prüfung pro Service) und ein Rate-Limit ergänzt
werden.

**Fehler**: 5xx-Antworten der Services werden 1:1 durchgereicht. Wenn ein Service gerade
hochfährt, sieht der Client kein 503 - der Interceptor puffert solange (s. nächster
Abschnitt). Erst wenn der Hochfahr-Vorgang in den Timeout läuft (30 s), gibt es ein
`504 Gateway Timeout`.

## KEDA - Automatisches Hoch- und Runterskalieren

**KEDA** ist eine Erweiterung für Kubernetes, die Pods anhand von echten Metriken
(z. B. HTTP-Requests pro Sekunde) hoch- und runterfahren kann. Das **HTTP Add-on** ist
ein Zusatzmodul, das speziell auf eingehende HTTP-Requests reagieren kann - auch dann,
wenn der Pod gerade gar nicht läuft.

**So funktioniert es im Projekt:**

- Im Ruhezustand laufen **0 Pods** pro Service. Bei 9 Services spart das ~3-5 GiB RAM,
  die sonst durch Idle-Container belegt wären.
- Kommt ein Request rein, hält der KEDA-Interceptor ihn fest und fährt das Deployment
  hoch (von 0 auf 1 Replica).
- Sobald der Pod bereit ist (`/readyz=200`), leitet der Interceptor den Request weiter.
  Der Client merkt nur eine etwas längere Antwortzeit - keine 503-Fehlermeldung.
- **Cold-Start** liegt für ein Rust-Binary bei ca. 3-5 Sekunden.
- Kommt **5 Minuten lang kein Request mehr**, fährt KEDA den Pod wieder runter. Alle
  Sessions im RAM des Services gehen damit verloren.

Pro Service gibt es ein `HTTPScaledObject` (das Pendant zum klassischen
`HorizontalPodAutoscaler`, aber für HTTP-Traffic) mit `min=0, max=1`.

## ArgoCD - GitOps

**ArgoCD** ist ein Werkzeug, das den Inhalt eines Git-Repos mit dem Cluster vergleicht
und Abweichungen automatisch korrigiert. Der Workflow:

- Jede Änderung an `helm/services/*` wird gepusht.
- ArgoCD erkennt die Änderung und deployt automatisch, ohne manuelles `helm install`.
- Ist ein Pod nicht im erwarteten Zustand (gelöscht, abgestürzt), zieht ArgoCD ihn wieder
  hoch (`selfHeal`).
- **Ausnahme**: ArgoCD ignoriert die Felder `replicas` und `resources` an den Deployments.
  Sonst würde es ständig versuchen, KEDA's „auf 0 skaliert"-Status auf den im Git stehenden
  Wert zu setzen.

Die Infrastruktur-Charts (Traefik, KEDA, Monitoring) werden **nicht** über ArgoCD
verwaltet, sondern bewusst manuell per `helm install` - damit beim Neuaufbau des Clusters
eine klare Reihenfolge eingehalten werden kann.

1. Gateway API CRDs
2. `traefik/` - API Gateway / LoadBalancer
3. `argocd/` - GitOps-Sync für alle weiteren Services
4. `monitoring/` - Prometheus + Grafana + Alertmanager + Tempo
5. `keda/` - KEDA Core (Operator + CRDs), muss VOR `keda-http/` kommen
6. `keda-http/` - KEDA HTTP Add-on (Interceptor) für Scale-to-Zero


## Monitoring

Drei Komponenten, die alle aus einem gemeinsamen Helm-Bundle kommen:

| Komponente   | Wozu                                                                         |
|--------------|------------------------------------------------------------------------------|
| Prometheus   | Sammelt Metriken (Request-Counter, CPU, RAM, Restarts, ...)                  |
| Tempo        | Sammelt **Traces** - einzelne Request-Verläufe inkl. Zeitstempel pro Schritt |
| Grafana      | UI, in der man Metriken und Traces visualisiert                              |

Die Services schicken ihre Daten automatisch über die Shared-Crates `metrics` (für
Prometheus) und `observability` (für Tempo). Im Grafana liegt ein vorgefertigtes
Dashboard `service-overview.json`, das alle Services in einer Übersicht zeigt (mit
Filter nach Namespace und Pod).

Die Dashboard-Queries aggregieren bewusst **nach Namespace** (also pro Service), nicht
pro Pod. Dadurch verschmelzen mehrere KEDA-Pod-Generationen eines Services zu einer
einzigen Linie und der Verlauf bleibt lesbar, auch wenn Pods ständig neu hochfahren (Pod name ändert sich jedes mal).

## 4.5 Build, Deployment, Tests

**Pipelines** (`.github/workflows/`, alle auf `ubuntu-latest`):

| Workflow      | Wann                  | Was                                                                                        |
|---------------|----------------------|--------------------------------------------------------------------------------------------|
| `ci.yml`      | Push auf Feature-Branch | Format-Check, Linter (Clippy), Tests mit Coverage                                          |
| `release.yml` | Push auf `main`      | Version bumpen -> Docker-Image bauen + nach ghcr.io pushen -> README + Helm-Values updaten |

Der Helm-Values-Update am Ende ist wichtig: Erst wenn das Image garantiert in der
Registry liegt, wird der Tag im Chart hochgezogen - sonst würde ArgoCD versuchen, ein
noch nicht existierendes Image zu deployen.

**Image-Build** (Dockerfile, multi-stage mit `cargo-chef`):

- `cargo-chef` ist ein Trick, der Rust-Dependencies separat von der eigenen Quellcode-
  Schicht baut. Bei Code-Änderungen ohne neue Dependency wird der teure Compile-Schritt
  aus dem Docker-Cache wiederverwendet. Builds gehen von ~10 min auf ~1 min runter.
- Das fertige Image basiert auf `debian:bookworm-slim` und ist ~100-200 MB groß.

**Tests:**

Jeder Service bringt seine eigenen Tests mit, was konkret geprüft wird, hängt von der
Fachlichkeit des UCs ab und steht in der jeweiligen UC-Sektion. Auf Pipeline-Ebene
ruft die CI für jeden Service einheitlich `cargo test --release -- --test-threads=1`
auf. Das `--release`-Flag ist Pflicht, weil FHE im Debug-Modus ~10× langsamer ist;
`--test-threads=1` verhindert dass parallele Tests den ServerKey-Speicher mehrfach
allokieren.

**Code Coverage** wird in der CI per `cargo-llvm-cov` pro Service ermittelt und als
Zeilen-Coverage-Badge ins jeweilige Service-README geschrieben. Sie ist nicht hart als
CI-Gate erzwungen - der Wert dient als Indikator, nicht als Block.
