# Test 3 — Happy-Flow

## Setup
- **Datum:** 2026-05-30
- **Skript:** `03_happy_flow.js`
- **Endpoint:** `http://159.195.145.100/leaderboard` (Image `v0.1.19`)
- **VUs/Iterations:** 1 VU × 20 sequentielle Iterationen

Pro Iteration: `GET /{code}/public-key` → 5× `POST /{code}/submit` → `GET /{code}/entries` → `POST /{code}/rank` (= 8 Requests).

## Ergebnisse

| | Wert |
|---|---|
| Laufzeit | 1 min 36 s |
| Fehlerrate | **0.00 %** (160/160 Checks) |
| Einzel-Request p50 | 332 ms |
| Einzel-Request p95 | 1.62 s |
| **Flow-Summe pro Iteration p50** | **4.6 s** |
| **Flow-Summe pro Iteration p95** | **5.9 s** |

## Beobachtung

- Ein vollständiger Spieler-Lebenszyklus dauert im Median 4.6 s, p95 5.9 s.
- Dominiert von den 5 Submit-Calls (FHE-`keep_max` ist der teuerste Pfad).
- `public-key`, `entries`, `rank` tragen kaum bei (Read-Lock + Base64, kein FHE).

## Reproduktion

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/happy-flow.json \
  03_happy_flow.js -e BASE_URL=http://159.195.145.100/leaderboard
```
