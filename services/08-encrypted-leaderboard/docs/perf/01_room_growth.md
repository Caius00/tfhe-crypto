# Test 1 — Room-Growth: Ergebnisse

## Setup
- **Datum:** 2026-05-29
- **Skript:** `01_room_growth.js`
- **Konfig:** `MAX_ROOMS=60`, `STAGGER_SEC=60` (1 neuer Raum/Min), `KEEPALIVE_SEC=540` (9 Min)
- **Endpoint:** `http://159.195.145.100/leaderboard` (Image `v0.1.18`)
- **Hardware:** Hetzner Dedicated, AMD EPYC 9645, 8 Cores, RAM hochgesetzt

## Verlauf
- **Laufzeit:** 58 min 7 s
- **Räume erfolgreich angelegt:** 57
- **Erste fehlgeschlagene Create:** VU 59 nach ~56 Min, Status `502 Bad Gateway`
- **Stopp-Ursache:** `http_req_failed > 5 %` (Threshold mit `abortOnFail`)

## Threshold-Ergebnisse

| Threshold | Ziel | Gemessen | Status |
|-----------|------|----------|--------|
| `http_req_duration` p95 | < 10 s | **6.41 s** | ✓ |
| `http_req_failed` | < 5 % | **5.20 %** | ✗ |

## Request-Statistik

| Wert | Anzahl |
|------|--------|
| Total HTTP-Requests | 269 |
| Erfolgreich | 255 (94.80 %) |
| Fehlgeschlagen | 14 (5.20 %) |
| Vollständige Iterationen | 155 |

## Latenzen — alle Requests (Create + Submit gemischt)

| Metrik | Wert |
|--------|------|
| min | 36.88 ms |
| median | 328.27 ms |
| p95 | 6.41 s |
| p99 | 12.45 s |
| max | 12.95 s |

## Latenzen — nur erfolgreiche Requests

| Metrik | Wert |
|--------|------|
| min | 36.88 ms |
| median | 328.27 ms |
| p95 | 6.33 s |
| p99 | 6.53 s |
| max | 7.44 s |

## Netzwerk

| Richtung | Volumen |
|----------|---------|
| Gesendet | 4.7 GB (≈ 57 × 80 MB ServerKey-Uploads + Submits) |
| Empfangen | 32 KB |

## Interpretation

- **Pod hat 57 parallele Räume mit Keepalive-Aktivität stabil gehalten.**
- **Kipp-Punkt liegt zwischen Raum 58 und 59** — VU 59 bekam beim `POST /create` ein `502 Bad Gateway`.
- p95 von 6.41 s liegt unter der 10-s-Spec-Schwelle — die Latenz war NICHT das Problem.
- 502er deuten auf Traefik-Timeout oder kurzzeitige Nicht-Erreichbarkeit des Pods (Memory-Pressure / GC-Pause). Der Pod erholte sich teilweise wieder — VU 56 und 57 konnten nach VU 59 noch Räume anlegen.
- Die 4.7 GB übertragene Daten kommen fast vollständig aus den ServerKey-Uploads (jeder ~80 MB) — die Submits selbst sind transportseitig billig.

## Zeitlicher Verlauf (erste Erstellung pro VU)

| Zeitpunkt (s) | VU | Raum-Code | Anmerkung |
|---------------|----|-----------|-----------|
| 67 | 1 | 957764 | erste Welle |
| 127 | 4 | 560665 | |
| 187 | 2 | 375066 | |
| 248 | 5 | 479673 | |
| 307 | 3 | 319222 | |
| 367 | 8 | 874408 | |
| 427 | 7 | 570336 | |
| 487 | 9 | 756540 | |
| 547 | 11 | 756499 | |
| 607 | 6 | 628746 | |
| … | … | … | je ~60 s ein neuer Raum |
| 3248 | 58 | 944947 | letzter Raum ohne Probleme |
| 3307 | 55 | 135095 | |
| **3368** | **59** | **—** | **erstes `502 Bad Gateway`** |
| 3428 | 56 | 731818 | trotz Fail davor wieder erfolgreich |
| 3488 | 57 | 664695 | letzter Raum vor Abort |

## Reproduktion

```bash
K6_PROMETHEUS_RW_SERVER_URL=http://localhost:9090/api/v1/write \
k6 run --out experimental-prometheus-rw \
  --summary-export=../results/room-growth.json \
  01_room_growth.js -e BASE_URL=http://159.195.145.100/leaderboard
```
