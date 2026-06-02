# Test 2 - Room-Fill

## Setup
- **Datum:** 2026-05-30
- **Skript:** `02_room_fill.js`
- **Konfig:** `MAX_PLAYERS=20`, `STAGE_DURATION_SEC=60` (10 Tempo-Stufen pro Runde von 10 s herunter auf 1 s), `PAUSE_DURATION_SEC=120` (zwischen Runden)
- **Endpoint:** `http://159.195.145.100/leaderboard` (Image `v0.1.19`)

1 Raum, Spielerzahl wächst Runde für Runde von 1 auf 20.

## Ergebnisse

| | Wert |
|---|---|
| Laufzeit | 4 h 0 min 17 s, alle 20 Runden vollständig durchgelaufen |
| HTTP-Requests | 25 268 |
| **Fehlerrate** | **0.02 %** (6 Timeouts in Runde 18, sofortige Recovery) |
| p50 (Submit) | 612 ms |
| p95 | 2.39 s |
| p99 | 4.52 s |

## Beobachtung

![Test 2 - p50/p95/p99 über die Zeit](test2_latency.png)

- **p50 (grün)** steigt gleichmäßig mit der Spielerzahl mit.
- **p95 (gelb)** zeigt **ab Runde 7 erste deutliche Sprünge**, die mit jeder weiteren Runde größer werden.
- **p99 (blau)** Spitzen bis 30 s ab Runde ~12, häufen sich Richtung Ende.
- Die Spitzen entstehen **systematisch zu Beginn jeder Runde**: nach der 2-min-Pause submitten alle aktiven Spieler quasi gleichzeitig wieder ihren ersten Wert, dadurch stauen sich kurz die FHE-Queue und der Background-Sort. Nach ein paar Sekunden desynchronisieren sich die Spieler und die Latenz normalisiert sich wieder.
- **Kein systemischer Crash.** Der Service bewältigt 20 parallele Spieler bei 1-s-Tempo, p95 bleibt insgesamt unter 3 s.

## Reproduktion

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/room-fill.json \
  02_room_fill.js -e BASE_URL=http://159.195.145.100/leaderboard
```
