# Loadtest — Encrypted Leaderboard

## 1. Vorbereitung (einmalig)

```bash
# FHE-Schlüssel + 200 Payloads erzeugen (~1–2 min)
cargo run --release -p encrypted-leaderboard --features loadtest \
  --bin gen_corpus -- --out services/08-encrypted-leaderboard/loadtest/corpus
```

## 2. Prometheus-Tunnel — brauche ich den?

**Optional, aber empfohlen.** Der Tunnel erlaubt k6, seine Client-Latenz live nach
Prometheus zu pushen → Dashboard zeigt dann Client- + Server-Sicht übereinander.

**Ohne Tunnel** läuft der Test trotzdem. Du siehst:
- k6-Summary im Terminal (p50/p95/p99)
- Server-Metriken im Dashboard (Prometheus scraped den Service intern)
- **Keine** Client-Latenz-Linie im Dashboard

```bash
# Tunnel öffnen (im Hintergrund, läuft bis Ctrl+C oder kill)
kubectl port-forward -n monitoring svc/prometheus-operated 9090:9090 &
```

Ohne Tunnel einfach `K6_PROMETHEUS_RW_SERVER_URL=...` und `--out experimental-prometheus-rw`
aus den Befehlen weglassen.

## 3. Tests

Jeder Test legt seinen Raum selbst an — **kein Vorab-Setup nötig**. Vor dem ersten Run:

```bash
cd services/08-encrypted-leaderboard/loadtest/k6
```

Dashboard mitschauen: <http://159.195.145.100/grafana/d/leaderboard-perf/>

### Test 1 — Happy-Flow (typische User-Latenz, ~5 min)

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/happy-flow.json \
  01_happy_flow.js -e BASE_URL=http://159.195.145.100/leaderboard
```

### Test 2 — Session-Crash (RAM-Decke, bis 30 min)

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/session-crash.json \
  02_session_crash.js -e BASE_URL=http://159.195.145.100/leaderboard
```

⚠ Dashboard offen halten — bei `node_memory_MemAvailable_bytes` < 500 MiB sofort Ctrl+C.

### Test 3 — Acceleration (Sättigungs-Test, 15 min × Variante)

```bash
# Variante A: 1 Spieler in 1 Raum
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/acceleration_A.json \
  03_acceleration.js -e BASE_URL=http://159.195.145.100/leaderboard \
  -e ROOMS=1 -e PLAYERS_PER_ROOM=1

# Variante B: 20 Spieler in 1 Raum
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/acceleration_B.json \
  03_acceleration.js -e BASE_URL=http://159.195.145.100/leaderboard \
  -e ROOMS=1 -e PLAYERS_PER_ROOM=20

# Variante C: 5 Räume × 20 Spieler
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/acceleration_C.json \
  03_acceleration.js -e BASE_URL=http://159.195.145.100/leaderboard \
  -e ROOMS=5 -e PLAYERS_PER_ROOM=20
```

## Korrektheit (Rust, lokal)

```bash
cargo test --release -p encrypted-leaderboard --test api -- --test-threads=1
```

## Output

- `corpus/` — generierte FHE-Daten (gitignored)
- `results/` — k6-Summary pro Run (gitignored)
- `../docs/perf/` — Screenshots + Tabellen für die Spec
