# Loadtest — Encrypted Leaderboard

## 1. Vorbereitung (einmalig)

```bash
# FHE-Schlüssel + 200 Payloads erzeugen (~1–2 min)
cargo run --release -p encrypted-leaderboard --features loadtest \
  --bin gen_corpus -- --out services/08-encrypted-leaderboard/loadtest/corpus
```

## 2. Prometheus-Tunnel

```bash
kubectl port-forward -n monitoring svc/prometheus-operated 9090:9090 &
```

Ohne Tunnel laufen die Tests trotzdem, aber im Dashboard fehlen die k6-Client-Linien.
In dem Fall einfach `K6_PROMETHEUS_RW_SERVER_URL=...` und `--out experimental-prometheus-rw`
aus den Befehlen weglassen.

## 3. Tests

Jeder Test legt seine Räume selbst an — kein Vorab-Setup nötig. Vor dem ersten Run:

```bash
cd services/08-encrypted-leaderboard/loadtest/k6
```

Dashboard mitschauen: <http://159.195.145.100/grafana/d/leaderboard-perf/>

### Test 1 — Room-Growth (wann ist der RAM voll?)

Jede Minute kommt ein neuer Raum mit einem Spieler dazu. Jeder Raum wird via
Keepalive-Submits alle 9 Min am Leben gehalten (Janitor evictet sonst nach
10 Min Idle). Test endet wenn der nächste `POST /create` scheitert oder
mehr als 5 % der Requests fehlschlagen.

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/room-growth.json \
  01_room_growth.js -e BASE_URL=http://159.195.145.100/leaderboard
```

Default-Konfig: 60 Räume max, 1 neuer pro Min, dann 30 Min halten. Anpassbar:
- `-e MAX_ROOMS=100`
- `-e STAGGER_SEC=30` (alle 30 s ein neuer Raum)
- `-e HOLD_DURATION=60m`

⚠ Dashboard offen halten — bei `node_memory_MemAvailable_bytes` < 500 MiB sofort Ctrl+C.

### Test 3 — Happy-Flow (typische User-Latenz, ~5 min)

1 Spieler macht 20× den vollen Ablauf: PubKey holen → 5 Scores einreichen
→ Liste lesen → Rang abfragen. Misst Summen-Latenz pro Durchlauf.

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/happy-flow.json \
  03_happy_flow.js -e BASE_URL=http://159.195.145.100/leaderboard
```

### Test 2 — Room-Fill (Acceleration + wachsende Spielerzahl)

1 Raum. Erst ein Spieler, der sein Submit-Tempo über 10 Stufen von „alle 10 s"
auf „jede Sekunde" steigert (1 min pro Stufe = 10 min Aktivität).
Dann 2 min Pause als Trennzeichen im Dashboard.
Dann kommt Spieler 2 dazu, beide laufen die 10 Stufen parallel.
Und so weiter, bis alle 20 Spieler aktiv sind.

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/room-fill.json \
  02_room_fill.js -e BASE_URL=http://159.195.145.100/leaderboard
```

Default-Laufzeit: **~4 Stunden** (20 Runden × 12 min). Abkürzbar:
- `-e MAX_PLAYERS=5` (nur bis 5 Spieler, ~1 h)
- `-e STAGE_DURATION_SEC=20` (kürzere Tempo-Stufen)
- `-e PAUSE_DURATION_SEC=30` (kürzere Pausen)

Smoke-Test (~12 min):
```bash
… 02_room_fill.js -e MAX_PLAYERS=3 -e STAGE_DURATION_SEC=20 -e PAUSE_DURATION_SEC=30
```

## Korrektheit (Rust, lokal)

```bash
cargo test --release -p encrypted-leaderboard --test api -- --test-threads=1
```

## Output

- `corpus/` — generierte FHE-Daten (gitignored)
- `results/` — k6-Summary pro Run (gitignored)
- `../docs/perf/` — Screenshots + Tabellen für die Spec
