# Spezifikation
**für 08-encrypted-leaderboard**
> [!NOTE]
> Pro umgesetztem Use Case sind die folgenden acht Sektionen verpflichtend. Die Sektionsstruktur ist für alle UCs identisch; die Detailtiefe darf je nach UC-Komplexität variieren (UC2 wird hier zwangsläufig weniger Inhalt haben als UC9).
---

### Funktionsbeschreibung

Der Service stellt ein verschlüsseltes Leaderboard für kleine Gruppen bereit, zum Beispiel eine Runde Freunde oder einen Tisch beim Quizabend. Spieler reichen ihre Scores verschlüsselt ein, der Server sortiert sie ohne jemals einen Klartext-Score zu sehen, und nur der Raum-Ersteller kann die Reihenfolge am Ende entschlüsseln.

**Akteure**

- **E (Initiator)** legt den Raum an. Er erzeugt lokal das TFHE-Schlüsselmaterial (ClientKey, ServerKey, PublicKey), behält den ClientKey für sich und lädt nur ServerKey und PublicKey zum Server hoch. Damit ist er der Einzige, der Ergebnisse später entschlüsseln kann.
- **Spieler** holen sich den PublicKey vom Server, verschlüsseln damit ihren Score und ihre Spieler-ID und schicken beides per Submit ein. Sie brauchen keinen eigenen Schlüssel.
- **Server** hält pro Raum eine Session mit dem dekomprimierten ServerKey, der Spieler-Liste (Score und ID als Ciphertext) und einer sortierten Sicht. Er kennt keinen Klartext-Score.

**Lebenszyklus einer Session**

1. **Raum anlegen:** E ruft `POST /create` mit seinem ServerKey und PublicKey auf und bekommt einen 6-stelligen Code zurück.
2. **Code teilen:** E gibt den Code out-of-band an seine Spieler weiter, also per Chat oder mündlich.
3. **Namen wählen (nur beim ersten Beitritt):** Der Spieler-Client zeigt einen einmaligen Name-Dialog (nur Buchstaben, 1 bis 20 Zeichen). Im Hintergrund werden eine UUID und ein zufälliges ID-Byte erzeugt. Beides wird in `localStorage` unter `lb_player_<code>` abgelegt. Bei einem späteren Beitritt in denselben Raum wird der Dialog übersprungen und die gespeicherte Identität wiederverwendet.
4. **Mitmachen:** Jeder Spieler ruft `GET /{code}/public-key` ab, verschlüsselt Score und ID-Byte lokal und sendet beides per `POST /{code}/submit` an den Server. Als `player_key` geht das Format `name:uuid` zum Backend. Die UUID dient nur der Eindeutigkeit für das Dedup und wird nirgendwo angezeigt. Reicht derselbe Spieler später erneut ein, behält der Server verschlüsselt das Maximum (`keep_max`).
5. **Sortieren:** Nach jedem Submit stößt der Server im Hintergrund eine Sortierung an (Batcher-Sortiernetzwerk). Pro Session läuft immer nur eine Sortierung gleichzeitig.
6. **Ergebnis abrufen:** E ruft `GET /{code}/entries` ab. Die Response enthält zwei Listen: die sortierten Ciphertexte (Score und ID) und ein Klartext-Roster mit `player_key` und `encrypted_id` pro Spieler. E entschlüsselt jeden ID-Ciphertext lokal mit seinem ClientKey, baut daraus die Tabelle `id-byte → name` und ordnet die Namen den sortierten Positionen zu.
7. **Schließen:** Es gibt keinen expliziten „Schließen"-Endpoint. Sessions werden vom Janitor nach 10 Minuten Inaktivität entfernt. Der Janitor ist ein Hintergrund-Task, der einmal pro Minute durch die Session-Map iteriert und alle Räume entfernt, an denen seit über 10 Minuten kein Request mehr eingegangen ist. Beim Entfernen wird der dekomprimierte ServerKey (mehrere hundert MB) freigegeben, andernfalls würde der Speicher volllaufen. Bei einem Server-Neustart gehen alle Sessions verloren. Es gibt keine Persistenz.

**Ablauf in Kurzform**

```
E                          Server                          Spieler
│                             │                               │
│── POST /create ────────────►│                               │
│◄────── { code } ────────────│                               │
│                                                             │
│── Code per Chat (out-of-band) ─────────────────────────────►│
│                             │                               │
│                             │            [Name-Dialog, einmalig pro Raum]
│                             │            [localStorage: {name, uuid, id-byte}]
│                             │                               │
│                             │◄──── GET /public-key ─────────│
│                             │────── { public_key } ────────►│
│                             │                               │
│                             │◄──── POST /submit ────────────│
│                             │      player_key = name:uuid   │
│                             │ [keep_max + Hintergrund-Sort] │
│                             │◄──── POST /submit … ──────────│
│                             │                               │
│── GET /entries ────────────►│                               │
│◄── entries + roster ────────│                               │
│                                                             │
│ E entschlüsselt jede ID, mappt roster → Namen pro Rang      │
```

### OpenAPI-Schnittstelle

Der Service stellt fünf fachliche Endpunkte unter dem Pfad-Prefix `/leaderboard` bereit. Die OpenAPI-Definition wird per `aide` aus dem Code generiert und ist live unter `GET /openapi.json` bzw. `GET /docs` (Swagger UI) verfügbar. Daneben gibt es noch `/healthz`, `/readyz`, `/version` und `/metrics` aus den shared Crates.

Das Body-Limit liegt bei 2 GiB (`DefaultBodyLimit::max(2*1024³)`), weil der komprimierte ServerKey je nach Parametern dreistellige Megabyte groß werden kann.

**`POST /create`** legt einen neuen Raum an.

Request:
```json
{
  "server_key": "<base64(bincode(CompressedServerKey))>",
  "public_key": "<base64(bincode(CompactPublicKey))>"
}
```
Response 200:
```json
{ "code": "847391" }
```
Status-Codes: `200` ok, `400` wenn `server_key` oder `public_key` kein gültiges Base64 oder die bincode-Deserialisierung fehlschlägt, `500` bei einer Panic im Dekomprimierungs-Thread.

**`GET /{code}/public-key`** gibt den Public-Key des Raums unverändert zurück, so wie er beim Anlegen hochgeladen wurde.

Response 200:
```json
{ "public_key": "<base64>" }
```
Status-Codes: `200`, `404` wenn der Raum nicht existiert oder bereits entfernt wurde.

**`POST /{code}/submit`** nimmt einen verschlüsselten Score entgegen.

Request:
```json
{
  "player_key": "Alice:550e8400-e29b-41d4-a716-446655440000",
  "encrypted_score": "<base64(bincode(FheUint16))>",
  "encrypted_id":    "<base64(bincode(FheUint8))>"
}
```
- `player_key` ist Klartext im Format `name:uuid`. Der Server nutzt den ganzen String als Dedup-Schlüssel, sodass ein Re-Submit derselben Identität ein `keep_max` auslöst. Die UUID sorgt für Eindeutigkeit, falls mehrere Spieler denselben Namen wählen, und wird nirgends im UI angezeigt.
- `encrypted_score` ist ein `FheUint16` (Score von 0 bis 65535).
- `encrypted_id` ist ein `FheUint8`, ein zufälliges ID-Byte (0 bis 255), das der Client pro Raum einmalig erzeugt. Es wandert mit dem Score durch den FHE-Sort und dient E später als Klartext-Label, um die sortierte Liste auf Namen zu mappen.

Response: `200` mit leerem Body.
Status-Codes: `200`, `400` bei ungültigem Base64 oder fehlgeschlagener bincode-Deserialisierung, `404` wenn der Raum unbekannt ist, `409` wenn der Raum voll ist (mindestens 20 Spieler und `player_key` ist neu), `500` bei FHE-Panic.

**`GET /{code}/entries`** liefert die verschlüsselte Reihenfolge plus das Roster.

Die Response besteht aus zwei Listen. `entries` ist die zuletzt fertig berechnete sortierte Sicht. Wenn noch kein Sort durchgelaufen ist, fällt `entries` auf die Insertion-Order zurück. `roster` ist ein Klartext-Verzeichnis aller bekannten Spieler im Raum (Insertion-Order) und ist unabhängig vom Sort-Status immer vollständig.

Response 200:
```json
{
  "entries": [
    { "encrypted_score": "<base64(FheUint16)>",
      "encrypted_id":    "<base64(FheUint8)>" },
    …
  ],
  "roster": [
    { "player_key":   "Alice:550e8400-…",
      "encrypted_id": "<base64(FheUint8)>" },
    …
  ]
}
```

Der Creator nutzt `roster`, um pro Spieler das ID-Byte zu entschlüsseln und sich daraus eine `byte → name`-Tabelle zu bauen. Mit dieser Tabelle bekommt jede sortierte Position in `entries` ihren Namen.

Status-Codes: `200`, `404` wenn der Raum unbekannt ist.

**`POST /{code}/rank`** fragt die Position einer verschlüsselten ID ab.

Request:
```json
{ "encrypted_id": "<base64(bincode(FheUint8))>" }
```
Response 200:
```json
{ "matches": ["<base64(FheBool)>", "<base64(FheBool)>", …] }
```
Pro Position der sortierten Liste kommt ein verschlüsselter Bool zurück (`true` heißt: die ID passt). E entschlüsselt jeden Bool lokal, und jede `true`-Position ist ein 1-basierter Rang. Mehrfachtreffer werden so automatisch unterstützt.
Status-Codes: `200`, `400` bei ungültigem Base64 oder fehlgeschlagener bincode-Deserialisierung, `404` wenn der Raum unbekannt ist, `500` bei FHE-Panic.

### Trust- und Threat-Model

Die Tabelle hat vier Spalten: *Datum*, *am Server klar*, *am Server verschlüsselt*, *nur am Client (E)*.

| Datum | klar | verschlüsselt            | nur Client              |
|---|---|--------------------------|-------------------------|
| ServerKey | ✓ (notwendig für FHE-Ops) |                          |                         |
| PublicKey | ✓ (wird an Spieler ausgeliefert) | ✓                        | ✓                       |
| ClientKey | |                          | ✓ (nur bei E)           |
| Score eines Spielers | | ✓ (`FheUint16`)          | nur entschlüsselt bei E |
| Spieler-ID (Listen-Position) | | ✓ (`FheUint8`)           | nur entschlüsselt bei E |
| `player_key` (Dedup-Token, `name:uuid`) | ✓ (z.B. `"Alice:550e8400-…"`) |                          |                         |
| UUID-Anteil von `player_key` | ✓ (client-erzeugt, im Klartext am Server) |                          | nie im UI sichtbar      |
| Raumcode | ✓ (6-stellig, im URL-Pfad) |                          |                         |
| Anzahl Spieler im Raum | ✓ (Listen-Länge) |                          |                         |
| Submit-Zeitpunkt | ✓ (Request-Timing) |                          |                         |
| Welche Position E abfragt (`/rank`) | | ✓ (`FheUint8` Target-ID) |                         |

**Was ein böswilliger Operator sehen kann** (reine Metadaten, kein FHE-Bruch):

- Wie viele Räume parallel existieren und wie viele Spieler in jedem sind.
- Wer (per `player_key` = `name:uuid`) wann postet, wie oft und in welchem Raum. Der Klartext-Name ist also sichtbar. Das ist gewollt, damit der Creator ihn im Leaderboard anzeigen kann. Der Server lernt dabei aber nicht, welcher Score zu wem gehört.
- Aus dem Timing der FHE-Operationen kann er ableiten, ob ein Spieler neu war (keine FHE-Op nötig) oder ob es sich um einen Re-Submit handelt (eine `keep_max`-Op). Der Klartext-Score bleibt trotzdem unsichtbar.
- Bei `/rank` sieht er, dass E gerade eine Rang-Abfrage macht, aber nicht, für welche Position.

**Restvertrauen in den Server**, das nicht an FHE hängt:

- Der Server liefert den richtigen PublicKey aus. Wenn er ihn austauscht, könnten Spieler unter Beobachtung verschlüsseln. Eine Absicherung dagegen gibt es in der aktuellen Version nicht. E muss dem Operator vertrauen, den Key korrekt weiterzureichen.
- Der Server führt die FHE-Ops korrekt aus und manipuliert keine Ciphertexts. Eine Verifizierbarkeit dieser Berechnung (per ZKP) ist nicht umgesetzt.
- Der Server liefert die richtige Liste zurück. Er könnte Einträge ignorieren oder duplizieren, und auch dafür gibt es keine Verifikation.

**Annahmen außerhalb von FHE**

- TLS liegt außerhalb des Service. In manchen Test-Umgebungen läuft es über reines HTTP. Das ist akzeptabel, weil ohnehin nur Ciphertexte über die Leitung gehen.
- TFHE-rs 1.6.1 wird als Black Box behandelt. CVE-mäßige Schwächen der Library werden hier nicht abgedeckt.
- Es gibt keine Authentifizierung. Wer den Raumcode kennt, kann submitten. Die Spieler-Zulassung wird sozial geregelt, also dadurch, dass E den Code nur an seine Gruppe weitergibt.
- Der `player_key` ist absichtlich Klartext, damit der Server Re-Submits desselben Spielers erkennen kann. Den Tausch eines Spielers gegen einen anderen kann der Server beobachten.

**Was nicht produktreif ist**

- Kein Schutz gegen einen aktiven Operator (Key-Tausch, Eintrags-Manipulation, Antwort-Fälschung).
- Keine Auth, kein Rate-Limit, keine Session-Persistenz.
- `MAX_ENTRIES = 20` ist eine harte Grenze, siehe §Performance.

**Was tatsächlich versprochen wird**

> *„Der Server kennt den Klartext-Score nicht. Er sieht nur Ciphertexte, vergleicht zwei Ciphertexte miteinander, ohne das Vorzeichen des Vergleichs zu kennen, und gibt die sortierte Liste an E zurück. Nur E kann die Liste mit seinem ClientKey entschlüsseln."*

Diese Garantie hält gegen einen ehrlich-aber-neugierigen Operator (*honest-but-curious*). Gegen einen aktiv manipulierenden Operator hält sie nicht.

### FHE-Designentscheidungen

Wir verwenden TFHE-rs 1.6.1 mit `ConfigBuilder::default()`, also die Standard-Parameter ohne eigenes Tuning. Für die hier gebrauchten Bitbreiten reicht das aus.

**Verwendete Typen**

- `FheUint16` für den Score (Wertebereich 0 bis 65535). Spielrelevante Scores wie Punkte oder Zeiten in Hundertstelsekunden lassen sich darauf vollständig abbilden. `FheUint8` (max 255) wäre zu eng, und `FheUint32` ist etwa doppelt so langsam pro Vergleich, ohne dass der erweiterte Wertebereich hier benötigt wird.
- `FheUint8` für die Spieler-ID. Bei `MAX_ENTRIES = 20` sind 0 bis 255 mehr als genug. Die ID dient nur als Tag, der zusammen mit dem Score durchsortiert wird, damit E nach dem Entschlüsseln weiß, wer auf welchem Platz steht.
- `FheBool` als Rückgabe von `/rank`, einer pro Position. Das ist der minimale Ciphertext, weil der Client nur „passt / passt nicht" lesen muss.

**Genutzte Operationen**

- `FheUint16::lt` ist der einzige Vergleich, den wir brauchen. Absteigend sortieren heißt: „tausche, wenn links kleiner ist".
- `FheBool::if_then_else` schaltet Score und ID synchron um, ohne dass der Server weiß, welcher Branch gewinnt. Das nutzen wir in `keep_max` (zweimal für Score, zweimal für ID) und im Sort-Comparator (viermal pro Vergleich).
- `FheUint8::eq` kommt nur in `rank_matches` zum Einsatz, dem Endpoint für die Positions-Abfrage.

Add, Mul und Bit-Shift brauchen wir nicht. Die Logik reduziert sich vollständig auf Vergleichen und datenabhängiges Auswählen.

**Verworfene Alternativen**

- `f32`/Float-Score wäre semantisch näher an „Punkten mit Nachkomma", aber TFHE-rs hat keine sinnvolle FHE-Float-Arithmetik, insbesondere keine Division. Wir bilden Hundertstel-Scores stattdessen auf `u16` ab (z.B. „12.34 s" wird zu 1234), das reicht für jede Quiz- oder Spiel-Skala.
- `FheUint32` für mehr Headroom wurde verworfen, weil jede zusätzliche Bitbreite die FHE-Vergleichszeit etwa verdoppelt und 16 Bit für diesen Use Case ausreichen.
- Klartext-Score mit ZKP (Spieler beweist „mein Score liegt im erlaubten Bereich") ist semantisch interessant, aber dann sieht der Server den Score. Das wollten wir vermeiden.
- Ein voller homomorpher Sort mit datenabhängigen Branches geht in TFHE-rs prinzipiell nicht, weil Branches auf Ciphertexts unmöglich sind. Die Lösung ist ein datenunabhängiges Sortiernetzwerk (siehe §Komplexität).
- Den `player_key` zu verschlüsseln wäre technisch möglich, aber unverhältnismäßig teuer. Der Server benötigt den Schlüssel, um beim Submit zu prüfen, ob bereits ein Eintrag desselben Spielers existiert, und gegebenenfalls `keep_max` auszuführen. Im Klartext geschieht das durch einen einfachen String-Vergleich in O(1). Verschlüsselt müsste der Server pro Submit jeden existierenden Eintrag homomorph vergleichen (`eq`) und jede Score- und ID-Position bedingt überschreiben (`if_then_else`). Hinzu kommt, dass der Server auf einem verschlüsselten Bool keine Verzweigung treffen kann, also nicht entscheiden kann, ob es überhaupt einen Treffer gab. Er müsste jeden Submit zugleich als neuen Eintrag anhängen und auf alle bestehenden Einträge bedingt anwenden. Damit steigen die Submit-Kosten von O(1) auf O(n) FHE-Operationen und die Listenlänge wäre nicht mehr beschränkbar. Der Privacy-Gewinn (Pseudonym statt Name) rechtfertigt diesen Aufwand nicht, da der Creator die Identität der Mitspieler ohnehin kennt.

**Approximationen.** Keine. Alle Operationen liefern exakte Resultate, solange die ServerKey-Parameter korrekt installiert sind. Es gibt kein Rauschen, das wir akzeptieren müssten, weil TFHE-rs intern bootstrappt und der Output deterministisch ist.

### Komplexität der eigenen Algorithmen

Wir betrachten n als Spielerzahl im Raum (höchstens 20) und zählen eine einzelne FHE-Operation als einen Schritt.

**Submit (`keep_max`).** Sendet ein Spieler einen neuen Score, obwohl bereits ein alter vorliegt, behält der Server verschlüsselt den höheren. Das sind unabhängig von der Belegung des Raums stets dieselben fünf FHE-Operationen, also O(1).

**Sortieren (`sort_by_score_desc`).** Nach jedem Submit muss die Liste neu sortiert werden. Auf verschlüsselten Zahlen kann der Server jedoch nicht die Größenrelation einsehen und auf dieser Grundlage entscheiden. Die Vergleiche müssen blind erfolgen, in einer festen Reihenfolge, die unabhängig vom Inhalt funktioniert. Dies leistet ein Sortiernetzwerk: eine vorab festgelegte Folge von Compare-and-Swap-Operationen der Form „Vergleiche zwei Positionen und tausche sie, falls links kleiner ist". Verwendet wird Batchers Odd-Even-Mergesort, da seine Vergleiche in Schichten organisiert sind, deren Paare disjunkt sind. Eine vollständige Schicht kann dadurch parallel auf mehreren CPU-Kernen ausgeführt werden. Der Aufwand beträgt O(n · log² n) Vergleiche insgesamt bei O(log² n) Schichten in der Tiefe. Für n = 20 ergibt das rund 120 Vergleiche in etwa 15 Schichten. Die Korrektheit ist im Test `batcher_layers_form_a_valid_sorting_network` per 0/1-Prinzip nachgewiesen.

Der Platzbedarf ist linear in n (die Liste der Ciphertexte). `keep_max` braucht nur konstanten Zusatzspeicher.

Da `keep_max` konstant ist und der aufwendige Sortierschritt im Hintergrund läuft, bleibt der Submit-Pfad schnell. Die O(n · log² n) der Sortierung sind der maßgebliche Grund für die feste Obergrenze `MAX_ENTRIES = 20`.

### Performance-Messung

**Mess-Setup.** Die Tests laufen auf einem NetCup-Server (AMD EPYC 9645, 8 Cores) gegen Image `v0.1.19`, TFHE-rs 1.6.1 mit `ConfigBuilder::default()`, `FheUint16` für den Score und `FheUint8` für die ID. Die Last erzeugt k6 mit vor-generierten Ciphertexten gegen `http://159.195.145.100/leaderboard`. Die Skripte liegen unter `loadtest/k6/`.

Gemessen werden die FHE-Endpunkte: `POST /create` (ServerKey-Decompress) und `POST /{code}/submit` (`keep_max` plus Background-Sort). Bewusst ausgeklammert sind `GET /entries`, `GET /{code}/public-key` und `POST /rank`, weil sie kein FHE machen und nur Grundrauschen liefern.

#### Test 1 - Room-Growth (2026-05-29)

Jede Minute wird ein neuer Raum mit einem Spieler und einem Submit angelegt, mit einem Keepalive alle 9 Minuten. Skript: `01_room_growth.js`.

| | Wert |
|---|---|
| p50 | 328 ms |
| p95 | 6.41 s |
| Fehlerrate | 5.20 % |
| **Kipp-Punkt** | **Raum 59 (`502 Bad Gateway`)** |
| Sessions vor Kipp | 57 stabil |

Die CPU-Last bleibt während des gesamten Tests nahezu konstant niedrig. Jeder Raum erzeugt nur ein `create` und alle 9 Minuten einen Keepalive-Submit, also keine nennenswerte FHE-Dauerlast. Dieser Test misst damit ausschließlich die RAM-Obergrenze durch Session-Akkumulation, nicht die Rechenleistung.

Volldaten: [`docs/perf/01_room_growth.md`](docs/perf/01_room_growth.md).

#### Test 2 - Room-Fill (2026-05-30)

Ein Raum, in dem die Spielerzahl in 20 Runden von 1 auf 20 wächst. Pro Runde gibt es 10 Tempo-Stufen (Sleep 10 s herunter auf 1 s) und danach 2 Minuten Pause. Skript: `02_room_fill.js`.

| | Wert |
|---|---|
| p50 (Submit) | 612 ms |
| p95 | 2.39 s |
| Fehlerrate | 0.02 % |
| Durchsatz Ø | 1.75 req/s über 4 h |
| **Kipp-Punkt** | **kein Crash, 4 h durchgelaufen** |
| **Erste p95-Sprünge** | **ab Runde 7 (= 7 Spieler)**, danach mit jeder Runde größer werdende Spitzen |
| **6 Timeouts** | Runde 18 (Sek 12 317), sofortige Recovery |

![Test 2 - p50/p95/p99 über die Zeit](docs/perf/test2_latency.png)

Im Latenz-Verlauf zeigt sich:
- p50 (grün) steigt gleichmäßig mit jeder zusätzlichen Spielerzahl.
- p95 (gelb) zeigt ab Runde 7 erste deutliche Sprünge, die mit jeder weiteren Runde größer werden.
- p99 (blau) hat Spitzen bis 30 s ab Runde 12, die sich am Ende häufen.

Die p95- und p99-Spitzen treten systematisch zu Beginn jeder Runde auf. Nach der zweiminütigen Pause setzen alle aktiven Spieler nahezu gleichzeitig ihren ersten Submit ab, wodurch sich die FHE-Queue und der Hintergrund-Sort kurz aufstauen. Innerhalb weniger Sekunden desynchronisieren sich die Spieler und die Latenz stabilisiert sich auf einem niedrigeren Niveau.

Volldaten: [`docs/perf/02_room_fill.md`](docs/perf/02_room_fill.md).

#### Test 3 - Happy-Flow (2026-05-30)

Ein Spieler durchläuft 20-mal sequentiell den Ablauf `public-key → 5× submit → entries → rank` (8 Requests pro Iteration). Skript: `03_happy_flow.js`.

| | Wert |
|---|---|
| Fehlerrate | 0.00 % (160/160 Checks) |
| **Flow-Summe p50** | **4.6 s** |
| **Flow-Summe p95** | **5.9 s** |

Volldaten: [`docs/perf/03_happy_flow.md`](docs/perf/03_happy_flow.md).

#### Throughput-Grenze

- **Parallele Sessions:** 57 stabil. Ab Raum 59 fällt der Service mit `502` aus.
- **Submit unter Dauerlast (4 h, bis 20 Spieler):** p95 gesamt 2.39 s, erste p95-Sprünge ab Runde 7 (siehe Test-2-Grafik), p50 steigt gleichmäßig mit der Spielerzahl, p99-Spitzen bis 30 s.
- **RAM pro aktiver Session:** etwa 350 bis 400 MB für den dekomprimierten ServerKey (komprimiert über die Leitung 80 MB).

#### Lastkurve

Im Grafana-Dashboard `leaderboard-perf` lässt sich der jeweilige Run über den Filter `testid` einblenden. Dort sind die per-Stufe-Latenzen und die Ressourcen über die Zeit ablesbar.

#### Reproduzierbarkeit

```bash
cargo run --release -p encrypted-leaderboard --features loadtest \
  --bin gen_corpus -- --out services/08-encrypted-leaderboard/loadtest/corpus

kubectl port-forward -n monitoring svc/prometheus-operated 9090:9090 &
cd services/08-encrypted-leaderboard/loadtest/k6

K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw 01_room_growth.js -e BASE_URL=http://159.195.145.100/leaderboard

K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw 02_room_fill.js -e BASE_URL=http://159.195.145.100/leaderboard

K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw 03_happy_flow.js -e BASE_URL=http://159.195.145.100/leaderboard
```

### Limitationen

**Eingabe-Grenzen.**
- Maximal 20 Spieler pro Raum (`MAX_ENTRIES`), bedingt durch die FHE-Sort-Komplexität O(n log² n).
- Maximal etwa 57 parallele Räume pro Service-Instanz, begrenzt durch den RAM-Verbrauch der Sessions (siehe Test 1).
- Sessions werden nach 10 Minuten Inaktivität entfernt. Es gibt keine Persistenz, ein Neustart entfernt sämtliche aktiven Sessions.

**Bewusst nicht umgesetzt.**
- Keine Authentifizierung. Die Spieler-Zulassung läuft über den Raumcode und ist sozial geregelt, analog zum Voting-Use-Case.
- Kein Rate-Limit auf den Submit-Endpoint.

**Technisch nicht machbar.**
- Keine Division, kein Float, keine datenabhängigen Branches in TFHE-rs. Score-Updates müssen sich auf `max()` reduzieren lassen.
- Der `player_key` muss im Klartext zum Server gehen, damit das Dedup serverseitig funktioniert. Das verrät, wer wann postet, lässt aber keinen Rückschluss auf den Score zu.
- Verliert E seinen ClientKey, ist die Liste nicht mehr lesbar. Recovery ist nicht möglich.

---