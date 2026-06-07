/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * stress.js – FHE Stress Test (Throughput-Grenze finden)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Isolierter Stress Test von POST /age-verification/.
 *   Dieser Endpunkt führt bei jedem Request eine vollständige FHE-Berechnung
 *   durch (ServerKey dekomprimieren + age_check). Da der Mutex während der
 *   gesamten Berechnung gehalten wird, werden Requests serialisiert.
 *
 *   Ziel: Herausfinden ab welcher parallelen Request-Anzahl p95 deutlich
 *   ansteigt und Timeouts auftreten.
 *
 * Endpunkte:
 *   - POST /age-verification/
 *
 * Voraussetzungen:
 *   1. Backend läuft auf dem Netcup-Server
 *   2. Payload generieren: cargo run --bin gen_payload
 *      → schreibt payload_age.txt und payload_sk.txt
 *
 * Ausführen:
 *   k6 run \
 *     --env BASE_URL=http://159.195.145.100/age-verification \
 *     --out json=results/stress.json \
 *     services/02-encrypted-age-verification/load-tests/stress.js
 *
 * Mess-Setup:
 *   - Tool:    k6 v2.0.0
 *   - TFHE:    ConfigBuilder::default()
 *   - Datum:   <vor dem Test eintragen>
 *   - Server:  Netcup KVM
 *   - CPU/RAM: <Serverspecs eintragen>
 *
 * Erwartetes Verhalten:
 *   - Bei 1–2 VUs: stabile Latenzen nahe Baseline
 *   - Ab 3–5 VUs: p95 steigt deutlich (Mutex serialisiert Requests)
 *   - Ab X VUs: Gateway Timeouts (499/504) durch Proxy-Timeout
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';
import { open } from 'k6/experimental/fs';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL      = __ENV.BASE_URL || 'http://159.195.145.100/age-verification';
const ENCRYPTED_AGE = open('./payload_age.txt');
const SERVER_KEY    = open('./payload_sk.txt');

// ── Payload ───────────────────────────────────────────────────────────────────
const payload = JSON.stringify({
    encrypted_age: ENCRYPTED_AGE,
    server_key: SERVER_KEY,
});

const params = {
    headers: { 'Content-Type': 'application/json' },
    timeout: '120s',
};

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const verifyLatency = new Trend('verify_latency', true);
const errorCount    = new Counter('errors');
const successRate   = new Rate('success_rate');

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    scenarios: {
        stress: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '30s', target: 1  },   // Baseline: 1 VU
                { duration: '60s', target: 3  },   // Leichte Last
                { duration: '60s', target: 6  },   // Mittlere Last (Mutex-Effekt erwartet)
                { duration: '60s', target: 10 },   // Peak: Throughput-Grenze
                { duration: '30s', target: 0  },   // Cool-down
            ],
            gracefulRampDown: '30s',
            exec: 'verifyFlow',
        },
    },
    thresholds: {
        'verify_latency': ['p(95)<120000'],  // 2 Minuten Limit
        'success_rate':   ['rate>0.90'],     // Toleranter wegen erwarteter Timeouts
    },
};

// ── Flow ──────────────────────────────────────────────────────────────────────
export function verifyFlow() {
    group('verify_age', () => {
        const res = http.post(`${BASE_URL}/`, payload, params);

        verifyLatency.add(res.timings.duration);

        const ok = check(res, {
            'status 200': r => r.status === 200,
            'has is_adult field': r => {
                try { return JSON.parse(r.body).is_adult !== undefined; } catch { return false; }
            },
        });

        successRate.add(ok);
        if (!ok) {
            errorCount.add(1);
            console.error(`Fehler: ${res.status} – ${res.body}`);
        }
    });

    sleep(5);
}