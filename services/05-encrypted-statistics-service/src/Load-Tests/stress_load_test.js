/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * stress_load_test.js – FHE Stress Test (Throughput-Grenze finden)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Isolierter Stress Test von POST /statistics/.
 *   Jeder Request löst eine vollständige FHE-Berechnung aus:
 *   sum, min, max, average (O(n)), median (O(n log²n) via Batcher-Netzwerk).
 *
 *   Da der FheEngine-Pool pro Request neu aufgebaut wird (eigener Rayon-Pool
 *   mit set_server_key im start_handler), gibt es keinen globalen Mutex wie
 *   bei UC02. Stattdessen ist die CPU der Engpass: Rayon nutzt alle Kerne,
 *   konkurrierende Requests kämpfen um CPU-Zeit.
 *
 *   Getestet mit n=10, bit_width=8.
 *
 * Endpunkte:
 *   - POST /statistics/
 *
 * Voraussetzungen:
 *   1. Backend läuft auf dem Netcup-Server
 *   2. Payloads generieren: cargo run --bin gen_payload
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
 *   - Bei 1 VU: Grundlatenz aus baseline_load_test.js
 *   - Ab 2–3 VUs: p95 steigt (CPU-Contention zwischen Rayon-Pools)
 *   - Ab X VUs: Proxy-Timeouts (499/504) durch Nginx-Timeout
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080/statistics';

const SERVER_KEY  = open('./payload_sk.txt');
const LIST_N10_B8 = open('./payload_list_n10_b8.txt');

const payload = JSON.stringify({
    encrypted_list: JSON.parse(LIST_N10_B8),
    server_key: SERVER_KEY,
    bit_width: 8,
});

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

// ── Flow ──────────────────────────────────────────────────────────────────────
export function stressFlow() {
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