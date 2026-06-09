/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * stress_load_test.js – FHE Stress Test (Throughput-Grenze finden)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Isolierter Stress Test von POST /.
 *   Jeder Request löst eine vollständige FHE-Berechnung aus:
 *   sum, min, max, average (O(n)), median (O(n log²n) via Batcher-Netzwerk).
 *
 *   Da der FheEngine-Pool jetzt SESSION-weit geteilt wird (nicht mehr pro
 *   Request neu aufgebaut), entfällt der ~80 MB Key-Upload-Overhead.
 *   Der eigentliche Engpass ist weiterhin CPU: Rayon nutzt alle Kerne,
 *   konkurrierende Requests kämpfen um CPU-Zeit.
 *
 *   Getestet mit n=10, bit_width=8.
 *
 * Session-Caching:
 *   setup() lädt den ServerKey einmalig via POST /session hoch.
 *   Alle VUs nutzen dieselbe session_id — kein Key-Upload pro Request.
 *
 * Endpunkte:
 *   - POST /session   (einmalig in setup)
 *   - POST /          (pro Iteration)
 *
 * Voraussetzungen:
 *   1. Backend läuft auf dem Netcup-Server (neuer Build mit Session-API)
 *   2. Payloads generieren (aus src/Load-Tests/):
 *        cargo run --bin gen_payload
 *      → schreibt payload_sk.txt und payload_list_n10_b8.txt
 *
 * Ausführen:
 *   k6 run \
 *     --env BASE_URL=http://159.195.145.100/statistics \
 *     --out json=results/stress.json \
 *     services/05-encrypted-statistics-service/src/Load-Tests/stress_load_test.js
 *
 * Mess-Setup:
 *   - Tool:    k6 v2.0.0
 *   - TFHE:    ConfigBuilder::default()
 *   - Datum:   <vor dem Test eintragen>
 *   - Server:  Netcup KVM
 *   - CPU/RAM: <Serverspecs eintragen>
 *
 * Erwartetes Verhalten:
 *   - Bei 1 VU: Grundlatenz aus baseline_load_test.js (deutlich weniger Traffic)
 *   - Ab 2–3 VUs: p95 steigt (CPU-Contention zwischen parallelen FHE-Ops)
 *   - Ab X VUs: Proxy-Timeouts (499/504) — aber Schwellwert höher als vorher,
 *     weil kein Key-Deserialisierungs-Overhead mehr pro Request
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080/statistics';

const SERVER_KEY  = open('./payload_sk.txt');
const LIST_N10_B8 = JSON.parse(open('./payload_list_n10_b8.txt'));

const params = {
    headers: { 'Content-Type': 'application/json' },
    timeout: '300s',
};

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const statsLatency = new Trend('stats_latency', true);
const errorCount   = new Counter('errors');
const successRate  = new Rate('success_rate');

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    scenarios: {
        stress: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '60s', target: 1  },
                { duration: '90s', target: 3  },
                { duration: '90s', target: 6  },
                { duration: '90s', target: 10 },
                { duration: '30s', target: 0  },
            ],
            gracefulRampDown: '30s',
            exec: 'stressFlow',
        },
    },
    thresholds: {
        'stats_latency': ['p(95)<300000'],
        'success_rate':  ['rate>0.90'],
    },
};

// ── Setup: ServerKey einmalig hochladen ───────────────────────────────────────
// Läuft einmal vor allen VUs. Rückgabewert wird an jede VU-Funktion übergeben.
export function setup() {
    const res = http.post(
        `${BASE_URL}/session`,
        JSON.stringify({ server_key: SERVER_KEY }),
        params
    );

    if (res.status !== 200) {
        throw new Error(`Session-Erstellung fehlgeschlagen: ${res.status} – ${res.body}`);
    }

    const { session_id } = JSON.parse(res.body);
    console.log(`Session erstellt: ${session_id}`);
    return { session_id };
}

// ── Flow ──────────────────────────────────────────────────────────────────────
export function stressFlow(data) {
    const payload = JSON.stringify({
        session_id: data.session_id,
        encrypted_list: LIST_N10_B8,
        bit_width: 8,
    });

    group('statistics', () => {
        const res = http.post(`${BASE_URL}/`, payload, params);

        statsLatency.add(res.timings.duration);

        const ok = check(res, {
            'status 200': r => r.status === 200,
            'has sum':    r => { try { return JSON.parse(r.body).sum    !== undefined; } catch { return false; } },
            'has median': r => { try { return JSON.parse(r.body).median !== undefined; } catch { return false; } },
        });

        successRate.add(ok);
        if (!ok) {
            errorCount.add(1);
            console.error(`Fehler: ${res.status} – ${res.body?.slice(0, 200)}`);
        }
    });

    sleep(5);
}
