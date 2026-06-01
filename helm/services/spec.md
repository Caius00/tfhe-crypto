# Services

## Verwendete Patterns

Bevor es ins Detail geht, ein Überblick über die Architektur-Patterns, an denen das
System aufgebaut ist:

- **Mono-Repo** - der gesamte Code (Rust-Services, Shared Crates, Angular-Frontend,
  Helm-Charts, CI-Workflows, Dokumentation) liegt in einem einzigen Git-Repo. Das
  erlaubt atomare Commits über mehrere Schichten hinweg (z. B. Service-Code + Helm-
  Konfiguration + README in einem PR) und vermeidet das Versions-Mismatch-Risiko
  mehrerer Repos.
- **Microservice-Pattern** - jeder Use Case ist ein eigener Service mit eigenem
  Lebenszyklus, eigenem Namespace und ggf. eigener Datenbank.
- **Cargo Workspace** (Rust) - ein einziges Cargo-Projekt enthält alle Services und
  geteilten Bausteine als Sub-Crates. Vorteil: gemeinsame `Cargo.lock`, ein
  `cargo build`-Aufruf kompiliert alles, Versionen sind zentral pinnbar.
- **Shared Library / Cross-Cutting Concerns** - wiederkehrende Aufgaben (Health,
  Metriken, Tracing, OpenAPI-Doku) liegen in `shared/`-Crates, werden per Pfad-
  Dependency in jeden Service eingehängt.
- **Code-First OpenAPI** - die OpenAPI-Spec wird nicht handgeschrieben, sondern
  zur Laufzeit aus den Rust-Typen generiert (via `aide` + `schemars`). DTOs im
  Code sind die Quelle der Wahrheit; Swagger-UI und JSON-Spec entstehen automatisch
  daraus.
- **Stateless-by-Default mit optionalem In-Memory-State** - die meisten Services sind
  zustandslos (jeder Request unabhängig). Wo ein UC eine Session braucht, lebt der
  State im RAM des Pods - nicht in der Datenbank, weil dort niemals Schlüssel oder
  noch-entschlüsselbare Ciphertexte liegen dürfen.
- **Path-based Routing** - Services kennen ihren öffentlichen Pfad nicht. Das Routing
  wird komplett am Gateway gemacht (siehe Infrastruktur).

## 4.1 Architektur-Übersicht

Das System besteht aus **neun unabhängigen Rust-Services**, einem Angular-Frontend und
einer geteilten Plattform. Jeder Use Case ist ein eigener kleiner Server, der nur sich
selbst kennt - keine zwei Services teilen sich Daten oder Schlüssel. Das macht
Entwicklung und Betrieb einfacher: stürzt ein Service ab, betrifft das die anderen nicht.

```
            ┌──────────────────────────────────────────────────┐
  Browser ──┼─► Angular Client (verschlüsselt lokal)           │
            └────────────┬─────────────────────────────────────┘
                         │  HTTP, Body enthält Base64-Ciphertexte
            ┌────────────▼─────────────┐
            │  Traefik Gateway         │   ← Hier endet das Klartext-Web
            └────────────┬─────────────┘
                         │
            ┌────────────▼──────────────┐    Keine FHE-Logik hier,
            │ KEDA Interceptor          │    nur Routing + Pod-Hochfahren
            └────────────┬──────────────┘
                         │
       ┌─────────────────┼─────────────────┐
       │                 │                 │
  ┌────▼────┐       ┌────▼────┐       ┌────▼────┐   ← Hier wird gerechnet
  │ Svc 01  │  ...  │ Svc 08  │  ...  │ Svc 09  │     (auf Ciphertexten)
  └────┬────┘       └─────────┘       └─────────┘
       │
  ┌────▼──────┐                       ┌───────────┐
  │ Redis     │   ← speichert nur     │ Postgres  │ (Svc 06)
  │ (Svc 01)  │     Bytes, weiß       │           │
  └───────────┘     nicht was drin    └───────────┘
                    steht
```

**Die FHE-Grenze**: Nur der Browser des Initiators und die Service-Pods sehen jemals
„echte" FHE-Werte. Alle anderen Komponenten (Gateway, Interceptor, Redis, Postgres)
arbeiten mit den verschlüsselten Bytes wie mit beliebigen anderen Daten - sie wissen
nicht, was drinsteht, und müssen es auch nicht.

## Workspace-Struktur

Auf Datei-Ebene sieht das Repo so aus:

```
tfhe-crypto/
├── Cargo.toml              # Workspace-Wurzel (listet alle Members + zentrale Deps)
├── client/                 # Angular Frontend (Branch: angular-client)
├── shared/
│   ├── health/             # Health-Endpunkte
│   ├── metrics/            # Prometheus-Exporter
│   ├── observability/      # OpenTelemetry / Tracing
│   └── openapi-docs/       # Swagger-UI Generator
├── services/
│   ├── 01-encrypted-key-value-store/
│   ├── 02-encrypted-age-verification/
│   ├── ...
│   └── 09-encrypted-program-execution/
├── helm/                   # Kubernetes-Deployments (siehe Infrastruktur-Spec)
│   ├── infrastructure/     # Cluster-Basis (Traefik, ArgoCD, KEDA, Monitoring)
│   └── services/           # Helm-Charts pro Service
└── .github/workflows/      # CI/CD
```

## Service-Inventar

| UC | Name                          | Pfad                | Datenbank |
|----|-------------------------------|---------------------|-----------|
| 01 | encrypted-key-value-store     | `/kv`               | Redis     |
| 02 | encrypted-age-verification    | `/age-verification` | -         |
| 03 | encrypted-voting-polling      | `/voting`           | -         |
| 04 | sealed-bid-auction            | `/auction`          | -         |
| 05 | encrypted-statistics-service  | `/statistics`       | -         |
| 06 | encrypted-genomics            | `/genomics`         | Postgres  |
| 07 | encrypted-image-processing    | `/image-processing` | -         |
| 08 | encrypted-leaderboard         | `/leaderboard`      | -         |
| 09 | encrypted-program-execution   | `/program-execution`| -         |

## Shared Crates (Gemeinsame Bausteine)

Jeder Service erledigt manche Dinge gleich (Health-Endpoints, Metriken, Tracing,
OpenAPI-Doku). Damit das nicht neunfach kopiert werden muss, liegen diese
Bausteine in `shared/` und werden pro Service eingehängt:

| Crate            | Was es liefert                                            |
|------------------|-----------------------------------------------------------|
| `health`         | `/healthz`, `/readyz`, `/version` - damit Kubernetes weiß, ob der Service läuft |
| `metrics`        | `/metrics`-Endpunkt mit Request-Zählern und Latenzen für Prometheus |
| `observability`  | Verbindet den Service mit dem Trace-Sammler (Tempo) - jeder Request wird sichtbar |
| `openapi-docs`   | Erzeugt automatisch eine `/docs`-Seite mit Swagger UI aus dem Rust-Code |

Ein neuer Service braucht diese vier Bausteine zusammen mit seiner fachlichen Logik -
mehr nicht.

## 4.3 Key Lifecycle (Was passiert mit den Schlüsseln)

Aus Sicht der Schlüssel gibt es genau zwei Rollen:

- **Initiator** - eine Partei mit dem ClientKey. Erstellt die Session, kann
  entschlüsseln.
- **Teilnehmer** - null bis beliebig viele weitere Parteien mit nur dem PublicKey.
  Können verschlüsseln, aber nicht entschlüsseln.

Wer in welcher Rolle ist, hängt vom jeweiligen UC ab und steht in dessen Sektion -
manche UCs kommen mit nur einer Partei (Initiator allein) aus, andere lassen viele
Teilnehmer zu.

In FHE gibt es drei Arten von Schlüsseln, die der Initiator erzeugt:

- **ClientKey** - der private Schlüssel zum Entschlüsseln. Bleibt **immer beim Browser**
  und wird **nie** an einen Server geschickt.
- **ServerKey** - erlaubt dem Server, auf verschlüsselten Daten zu rechnen, ohne sie
  selbst entschlüsseln zu können. Wird beim Erstellen der Session hochgeladen.
- **PublicKey** - kann an Teilnehmer weitergegeben werden, damit sie verschlüsseln
  können, ohne den ClientKey zu kennen. Wenn ein UC keine Teilnehmer hat, bleibt
  der PublicKey ungenutzt.

**Konzeptueller Datenfluss** (Details, Endpunkt-Namen und ob es überhaupt einen
Session-Begriff gibt, hängen vom UC ab):

1. **Schlüsselerzeugung** - der Initiator erzeugt client-seitig alle drei Schlüssel.
   Der ClientKey verlässt den Client nie.
2. **ServerKey-Übertragung** - der Initiator schickt den ServerKey zum Service.
3. **Dekompression** - der Service dekomprimiert den ServerKey. Da das mehrere
   Sekunden CPU-Zeit und mehrere hundert MB RAM kostet, läuft es in einem
   Hintergrund-Thread und nicht im HTTP-Handler.
4. **Eingaben** - werden verschlüsselt an den Service geschickt (vom Initiator
   selbst mit ClientKey oder PublicKey, oder von Teilnehmern mit dem PublicKey).
5. **Berechnung** - der Service rechnet auf den verschlüsselten Werten.
6. **Ergebnis** - kommt verschlüsselt zurück. Nur der Initiator kann es mit seinem
   ClientKey entschlüsseln.

**Lebensdauer**: Schlüssel und sonstiger Sitzungs-State leben ausschließlich im RAM des
Service-Pods. Auf der Festplatte oder in einer Datenbank landen nur fachliche
Klartext-Metadaten, niemals Schlüssel oder Ciphertexte, die noch jemand entschlüsseln
könnte. Wenn der Pod neu startet oder von KEDA auf 0 skaliert wird, ist alles im RAM
weg - die betroffene Partei muss von vorn anfangen. Einige UCs ergänzen das um
zusätzliche Aufräum-Mechanismen (z. B. Idle-Eviction einzelner Sessions).

## 4.4 Serialisierung

FHE-Ciphertexte sind im Kern Byte-Arrays, die nicht als JSON-Zahl darstellbar sind.
Die Wahl, wie sie übers Netz transportiert werden, ist überall gleich:

| Schicht                                | Wie                       |
|----------------------------------------|---------------------------|
| Ciphertext intern (Service ↔ Service)  | `bincode` (binäres Format) |
| Ciphertext über HTTP                   | Base64-kodierter Bincode  |
| Request/Response-Hüllen (DTOs)         | normales JSON via `serde` |
| OpenAPI-Spec für Swagger UI            | automatisch aus Rust-Typen |

**Warum Base64?** Damit der Ciphertext in einem JSON-String-Feld liegen kann - JSON
verträgt keine rohen Bytes. Kostet ~33 % Overhead, ist aber browserseitig trivial zu
de-/kodieren.

**Typische Größen** (TFHE-rs Standard-Konfiguration):

| Typ                    | Bytes (bincode) | Bytes (base64) |
|------------------------|-----------------|----------------|
| `FheBool`              | ~50 KB          | ~67 KB         |
| `FheUint8`             | ~50 KB          | ~67 KB         |
| `FheUint16`            | ~100 KB         | ~134 KB        |
| `FheUint64`            | ~400 KB         | ~534 KB        |
| `CompressedServerKey`  | ~100 MB         | ~134 MB        |
| `PublicKey`            | ~50 KB          | ~67 KB         |

Das ist relevant für die HTTP-Endpunkte, die einen ServerKey entgegennehmen: Axum cappt
den Body sonst bei 2 MB. Daher ist in den Services explizit ein **2 GiB Body-Limit**
gesetzt, damit der Upload durchgeht.
