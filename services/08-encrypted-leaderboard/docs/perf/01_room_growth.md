# Test 1 - Room-Growth

## Setup
- **Datum:** 2026-05-29
- **Skript:** `01_room_growth.js`
- **Konfig:** `MAX_ROOMS=60`, `STAGGER_SEC=60` (1 neuer Raum pro Minute), `KEEPALIVE_SEC=540` (alle 9 Minuten ein Submit pro VU, damit der Janitor die Session nicht entfernt)
- **Endpoint:** `http://159.195.145.100/leaderboard` (Image `v0.1.19`)

## Ergebnisse

| | Wert |
|---|---|
| Laufzeit | 58 min 7 s (Abort durch `abortOnFailure`) |
| Max. parallele Sessions | **57** |
| **Kipp-Punkt** | **Raum 59 - `502 Bad Gateway`** |
| p50 | 328 ms |
| p95 | 6.41 s |
| Fehlerrate | 5.20 % |

## Beobachtung

- Der Service hält 57 parallele Sessions stabil; ab Session 59 bricht die Verfügbarkeit ein.
- p95 bleibt unter Last mit 6.41 s unterhalb der 10-Sekunden-Spezifikationsschwelle. Die Latenz war nicht der limitierende Faktor, sondern die Service-Verfügbarkeit (Memory-Pressure und Connection-Reset).
- Die CPU-Last bleibt während des gesamten Tests nahezu konstant niedrig: pro Raum nur ein `create` und alle 9 Minuten ein Keepalive-Submit. Dieser Test misst damit ausschließlich die RAM-Obergrenze, nicht die Rechenleistung.
- RAM pro Session: etwa 350-400 MB dekomprimiert, 80 MB komprimiert über die Leitung.

## Reproduktion

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/room-growth.json \
  01_room_growth.js -e BASE_URL=http://159.195.145.100/leaderboard
```
