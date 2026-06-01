# Architekturentscheidungen

Begründung der wesentlichen Technologie- und Design-Entscheidungen. Pro Punkt:
**was wurde gewählt, warum, und welche Alternative gab es**.

## Anwendungsarchitektur & Rust-Backend

### Microservices

Statt einem einzigen großen Backend ist jeder Use Case ein eigenständiger Service.
Vorteile: jeder UC hat eigenen Lebenszyklus (Deployments, Versionen), Isolation auf
Prozess-Ebene, klare Verantwortungs-Grenzen für Abgaben und Tests. Beim FHE-Setup
besonders wichtig, weil jeder Service seinen eigenen großen ServerKey im RAM hält -
wäre das ein Monolith, würde jede Session die Speicher-Bilanz aller UCs zugleich
beeinflussen. Nachteil: Operations-Overhead (9 Pipelines, 9 Charts, 9 Namespaces),
was wir bewusst in Kauf nehmen.

Verworfen: **Modular-Monolith**. Wäre weniger Infrastruktur-Aufwand, aber alle FHE-
Daten lebten im selben Prozess - Fehler in einem UC könnten andere zum Absturz
bringen. Außerdem klarer didaktischer Wert in der Microservice-Trennung.

### Warum Shared Crates statt Copy-Paste?

Wiederkehrende Bausteine (Health-Endpunkte, Prometheus-Metriken, Tracing, OpenAPI-
Doku) liegen in `shared/`-Crates und werden per Pfad-Dependency in jeden Service
eingehängt. Ändert sich ein Verhalten an einer dieser Stellen (z. B. neuer Trace-
Exporter), reicht eine Änderung im Shared-Crate und alle Services profitieren beim
nächsten Build.

Verworfen: **Code-Kopie pro Service**. Schneller anfangs, aber führt nach
spätestens drei Services zu Inkonsistenzen und Fix-Drift.

### Warum Cargo Workspace / Monorepo?

Ein einziges Cargo-Projekt enthält alle Services und Shared-Crates als Sub-Crates.
Vorteile: gemeinsame `Cargo.lock` (alle Services nutzen dieselben Abhängigkeits-
Versionen), `cargo build` kompiliert alles, Crate-zu-Crate-Refactorings sind atomar
in einem Commit möglich. Das gesamte Repo (Rust-Services + Helm-Charts + Frontend +
CI-Workflows) liegt zusätzlich als Mono-Repo, damit Service-Änderung und Deployment-
Konfiguration in einem PR zusammengehören.

Verworfen: **ein Repo pro Service** (Polyrepo). Würde Versions-Mismatch zwischen
Shared-Crates erlauben und cross-cutting Refactorings zur Submodule-Hölle machen.

### Warum Axum?

Axum ist das HTTP-Framework, das aktuell zur Tokio-Familie gehört und auf dem
Tower-Middleware-Stack aufsetzt. Vorteile: kleines Kern-API, idiomatische
Extractors (`State`, `Path`, `Json`), gute Integration mit `tower-http` für
Tracing/Limits/Compression. Wird aktiv von der Tokio-Org gepflegt.

Verworfen: **Actix-web** (eigene Actor-Runtime, mehr Magic), **Rocket** (weniger
flexibel bei Middleware), **warp** (filter-Kombinatoren funktional schick, aber
typtechnisch sperrig).

### Warum OpenAPI-Generierung aus Code (aide) statt manuelle Spec?

`aide` + `schemars` leiten die OpenAPI-Spec zur Laufzeit aus den Rust-DTOs ab.
Vorteile: Code ist die Single Source of Truth - eine Schema-Drift zwischen Spec
und Implementierung kann nicht passieren. Swagger-UI ist auf jedem Service unter
`/docs` automatisch verfügbar.

Verworfen: **manuelle YAML-Spec** + Codegen daraus. Macht Sinn bei
Schema-First-Workflows mit mehreren Implementierungs-Sprachen, hier aber Overhead.
**utoipa** wäre eine vergleichbare Alternative; `aide` wurde gewählt weil es
besser zum axum-`ApiRouter`-Pattern passt und scheinbar leichter zu implementieren ist.

## FHE-Kryptographie-Architektur

### Warum opt-level = 3 nur für die tfhe-Crate?

`tfhe-rs` ist im Debug-Build praktisch unbenutzbar - eine Schlüsselgenerierung
dauert dann Minuten statt Sekunden, FHE-Operationen sind ~10× langsamer. Wir
setzen daher in der Workspace-`Cargo.toml`:

```toml
[profile.dev.package.tfhe]
opt-level = 3
```

Damit wird **nur** die `tfhe`-Crate selbst optimiert; der eigene Code bleibt im
Debug-Modus (Breakpoints, Logs, schnelle Kompilierung).

Verworfen: **alles im Release-Modus testen**. Macht alle eigenen Crates auch beim
Entwickeln langsam und unterbindet Debug-Komfort. Mit dem Profile-Override haben
wir das Beste aus beiden Welten.

### Warum target-cpu=native im Docker-Build?

`tfhe-rs` nutzt CPU-spezifische SIMD-Instruktionen (AVX2, AVX-512), wenn der
Compiler weiß, dass sie verfügbar sind. Da das Image für genau einen bekannten
Server (Hetzner AMD EPYC) gebaut wird, ist `target-cpu=native` sicher und liefert
deutlich bessere FHE-Performance als der konservative x86_64-Default.

Verworfen: **portables x86_64-Image**. Wäre auf beliebigen Hosts lauffähig, aber
spürbar langsamer beim Bootstrapping und bei Multiplikationen.

## Frontend-Architektur

### Warum Angular (mit TFHE.js)?

Die Entscheidung fiel pragmatisch: ein **Rust-Frontend** wäre möglich gewesen
(z. B. mit Leptos oder Dioxus), aber für ein Team mit gemischter Erfahrung zu
aufwändig. Für JavaScript/TypeScript gibt es mit **TFHE.js** eine offizielle
WASM-Variante der TFHE-Bibliothek, die FHE-Operationen direkt im Browser erlaubt -
damit war TypeScript als Frontend-Sprache gesetzt.

Innerhalb der TypeScript-Welt fiel die Wahl auf **Angular**, weil mehrere
Team-Mitglieder bereits Angular-Erfahrung hatten - das spart Einarbeitungszeit
und bringt einheitliche Konventionen (Routing, Forms, HTTP-Client,
Dependency-Injection sind im Framework selbst).

Verworfen: **Rust-Frontend** (zu hoher Lernaufwand für das Team),
**React/Vue/Svelte** (keine ausgeprägte Team-Erfahrung).

## DevOps, Cloud Native & Infrastruktur

### Containerisierung

#### Warum ein Image pro Service statt einem gemeinsamen Image?

Erst die Architektur-Entscheidung, dann (im nächsten Punkt) wie der einzelne
Build optimiert wird.

Pro Service ein eigenes Docker-Image. Hauptgrund: die 9 Services sollen
**unabhängig voneinander entwickelt und betrieben** werden können - getrennte
Lifecycles, getrennte Tests, getrennte Deployments. Crasht ein Pod, laufen die
anderen acht ungestört weiter. Kubernetes startet nur den betroffenen wieder
neu (in Bruchteilen einer Sekunde für ein Rust-Binary).

Nachteil: ressourcenintensiver, weil 9 Pods statt einer existieren. Wird aber
durch **KEDA Scale-to-Zero** weitgehend entschärft. Nicht-genutzte Services
laufen auf 0 Replicas und kosten gar nichts. Die Speicher-Bilanz im Idle-Zustand
liegt damit nahe an dem, was ein monolithisches Image hätte.

Verworfen: **Monolithisches All-in-One-Image**. Wäre etwas effizienter (ein
HTTP-Server statt neun), würde aber alle 9 Services in einen gemeinsamen Build,
eine gemeinsame Dependency-Version und einen gemeinsamen Deploy zwingen. Ein
einziger Bug in der HTTP-Schicht würde alle UCs gleichzeitig kippen, jedes
Update alle Pods restarten.

Ebenfalls verworfen: **dedizierter FHE-Pod als gemeinsamer Backend-Service**.
Dabei wäre die `tfhe`-Bibliothek nur in einem einzigen Pod gelandet, die 9 UC-
Services hätten ihn per RPC angesprochen. Vorteil: drastisch kleinere
UC-Images, ServerKey nur einmal im RAM. Nachteil: die UC-Services hätten alle
auf eine gemeinsame Schnittstelle warten müssen - so lange die nicht stand,
wäre die Entwicklung im ganzen Team blockiert gewesen. Außerdem würde der
FHE-Pod zum Single-Point-of-Failure für sämtliche Use Cases.

#### Warum Multi-Stage Build mit cargo-chef?

Weil jeder Service sein eigenes Image bekommt (siehe oben), muss der Image-Build
schnell sein - sonst wartet die Pipeline pro Push minutenlang auf bis zu 9
parallele Compile-Läufe.

`cargo-chef` trennt die Build-Stages: `planner` extrahiert einen reinen
Dependency-Plan aus dem Cargo-Manifest, `cacher` kompiliert nur die
Dependencies, `builder` legt den eigenen Code darüber. Solange sich keine
Dependency ändert, kommt der teure Compile-Schritt aus dem Docker-Layer-Cache -
Builds gehen einige Minuten schneller.

Verworfen: **einfacher Single-Stage Build**. Funktioniert, kompiliert aber bei
jeder Code-Änderung alle Dependencies neu. Bei 9 Services pro Push nicht
tragbar.

### Container-Orchestrierung (Kubernetes)

#### Warum Kubernetes? (Docker-Compose vs k3s vs vanilla k8s)

Multi-Service-Setup mit Service-Discovery, Self-Healing, einheitlicher
Deploy-Pipeline und horizontaler Skalierung. Compose würde nur lokal skalieren
und kennt keine Auto-Heal-Semantik. Vanilla-Kubernetes wäre für einen
Single-Node-Cluster überdimensioniert (Server nicht genug Leistung).

Gewählt: **k3s**. Eine einzige Binary, dieselben APIs wie vanilla k8s, deutlich
weniger Overhead, ideal für Single-Node-Setups auf einem Server.

#### Warum ArgoCD / GitOps statt direktem kubectl apply?

GitOps-Prinzip: der Soll-Zustand des Clusters liegt im Git-Repo, ArgoCD
vergleicht laufend und gleicht ab. Vorteile: jede Cluster-Änderung ist im Git
nachvollziehbar (Commit-History), Rollback per Git-Revert, automatisches
Self-Healing bei verschwundenen Pods.

Verworfen: **manuelles `kubectl apply` aus der Pipeline**. Funktioniert, aber
ohne Drift-Erkennung - wer manuell etwas am Cluster ändert, fällt aus dem Bild.
**Flux** als Alternative wäre vergleichbar; ArgoCD wurde wegen der besseren UI
gewählt (manuelle Eingriffe wie temporäres Replicas-Hochsetzen sind viel
einfacher).

#### Warum Traefik als API-Gateway statt NGINX oder direkter NodePort?

Traefik unterstützt die offizielle Kubernetes Gateway API (inkl. TCPRoute, das
wir für externen Redis-/Postgres-Zugang brauchen), hat eine eingebaute
Auto-Discovery von Routen und ist mit einem einzigen Helm-Chart aufgesetzt.

Verworfen: **NGINX-Ingress** (kein TCPRoute-Support in der Standard-Distribution),
**direkter NodePort** (keine Pfad-basierte Routing-Logik, keine TLS-Terminierung
zentralisierbar).

#### Warum Helm Charts?

Helm ist der De-facto-Paket-Manager für Kubernetes (vergleichbar mit `apt` oder
`npm` für Linux/Node). Vorteile: Templating mit Werten, Wrapper um offizielle
Community-Charts (Traefik, ArgoCD, KEDA, kube-prometheus-stack) ohne deren
Manifeste selbst pflegen zu müssen, klare Versionierung.

### CI/CD Automatisierung

#### Warum pfad-basierte Change Detection statt immer alle Services?

Bei 9 Services würde ein „alles bauen"-Workflow nach jedem Push 9 parallele
Compile-Jobs starten - selbst mit cargo-chef-Cache mehrere Minuten Verbrauch
pro Push. Stattdessen erkennt `dorny/paths-filter@v3` welche Service-Pfade
geändert wurden und baut nur diese.

Verworfen: **immer alles bauen**. Einfacher zu konfigurieren, aber Verschwendung
von CI-Minuten.

#### Warum Clippy als Pflicht-Gate im CI?

`clippy` ist der offizielle Rust-Linter und prüft auf Idiom-Verletzungen,
potenzielle Bugs (z. B. Vergleich von Floats, unnötige Clones) und Style-Issues.
Im CI als `-D warnings` aktiviert - jede Clippy-Warnung blockiert den Merge.

Verworfen: **Clippy nur lokal**. Funktioniert solange jeder es ausführt - tut
in der Praxis nicht jeder. Pipeline-Gate ist die einzige verlässliche Variante.

#### Warum Tests im Release-Modus und mit --test-threads=1?

`--release` ist wegen `tfhe-rs` Pflicht: im Debug-Modus dauert ein FHE-
Integrationstest 10× länger (Bootstrapping-Operation, die im Debug
unoptimiert ist). `--test-threads=1` verhindert dass mehrere Tests parallel
einen ServerKey dekomprimieren - das frisst pro Decompress mehrere Hundert MB
RAM und sprengt den Speicher der CI-Runner.

Verworfen: **Debug + parallel** (Default-Verhalten von `cargo test`). Tests
liefen Minuten statt Sekunden und OOM-Killten den Runner sporadisch.

#### Warum cargo-llvm-cov + nextest statt cargo test?

`nextest` ist ein moderner Test-Runner (deutlich schneller dank Parallelisierung
und besserer Lifecycle-Verwaltung) mit hilfreicheren CLI-Ausgaben. `cargo-llvm-cov`
ist der zuverlässigste Coverage-Tool für Rust, weil er auf LLVM-Instrumentierung
basiert - genaue Zeilen-/Branch-Coverage, kein Drift gegenüber dem tatsächlichen
Compile-Output.

Verworfen: **`cargo test`** (langsamer, schlechtere Ausgabe), **`cargo-tarpaulin`**
(älter, ungenauer auf modernen Rust-Versionen).

### Release & Versionierung

#### Warum automatischer Patch-Bump bei Merge nach main?

Jeder Merge nach `main` bekommt automatisch einen Patch-Bump (`0.1.x → 0.1.x+1`).
Umgesetzt direkt in `.github/workflows/release.yml` mit einem kleinen
Shell-Skript: aktuelle Version aus der jeweiligen Service-`Cargo.toml` lesen
(`grep` + `cut`), Patch-Komponente per Bash-Arithmetik inkrementieren, per
`sed` zurückschreiben und anschließend `cargo update -p <service> --precise <neu>`
ausführen, damit die Workspace-`Cargo.lock` synchron bleibt.

Vorteil: Versionen sind monoton und nachvollziehbar, der entsprechende
Docker-Image-Tag landet in der Registry, Helm-Values-Datei wird automatisch
upgedatet, ArgoCD deployt. Manuelle Minor-/Major-Bumps macht man dann bewusst
in der Service-`Cargo.toml`.

Verworfen: **manuelles Tagging** (funktioniert, aber leicht vergessen - dann
hat man Container-Images mit Hash-Tags, die kein Mensch zuordnen kann),
**release-plz** (Cargo-Plugin für automatische Releases, war anfangs im Einsatz
- Spuren davon sind noch in Form alter Git-Tags wie `encrypted-*-v0.1.0` im
Repo; passte aber nicht zu unserer Multi-Service-Matrix mit zentralem Helm-
Update, weshalb auf das eigene Skript umgestellt wurde).

#### Warum GHCR statt DockerHub?

GHCR (GitHub Container Registry) ist direkt am GitHub-Repo angebunden: dieselben
Credentials (`GITHUB_TOKEN`), dieselbe Sichtbarkeit (privates Repo → privates
Image), keine zusätzlichen Logins in der Pipeline nötig. DockerHub hätte einen
separaten Account, separate Quoten und eigene Rate-Limits gebraucht.

## Querschnitt: Scale-to-Zero & Observability

### Warum KEDA HTTP Add-on statt Knative Serving?

Beide bieten Scale-to-Zero für HTTP-Workloads. KEDA ist deutlich leichter (~3 Pods
für die ganze Infrastruktur) und integriert sich sauber mit der vorhandenen
Traefik-Gateway-API-Konfiguration. Knative würde eigene Networking-Schicht
mitbringen (Kourier/Istio/Contour), zusätzliche ~7-10 Pods + Queue-Proxy-Sidecar
pro Service-Pod - bei einem 16-GiB-Cluster zu viel Overhead.

Verworfen: **Knative Serving**. Branchenstandard für „Serverless auf k8s", aber
für unsere Cluster-Größe und Anzahl Services überdimensioniert.

### Warum Prometheus + Grafana + Tempo statt eines Fertig-SaaS?

Open-Source-Stack, läuft komplett im Cluster, keine externen Abhängigkeiten,
keine Daten-Auslieferung an Drittanbieter. Tempo ist Teil der Grafana-Familie -
Traces erscheinen direkt im selben UI wie Metriken, kein zweites Frontend nötig.

Verworfen: **Datadog/New Relic/Honeycomb**. Hervorragende UIs, aber für ein
Hochschulprojekt zu teuer und unnötig vendor-locked. Self-Hosted Jaeger statt
Tempo wäre möglich, hätte aber eine eigene Trace-UI getrennt von Grafana.
