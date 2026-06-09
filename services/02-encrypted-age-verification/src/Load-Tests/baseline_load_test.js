/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * performance_tests.js – Age Verification Last- und Stresstest (session-basiert)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Session-basierte Variante: ServerKey wird einmalig per POST /session
 *   hochgeladen. Alle Requests danach schicken nur noch encrypted_age (~88 KB).
 *
 *   - Szenario A: Baseline (1 VU, sequentiell) – misst reine FHE-Grundlatenz
 *   - Szenario B: Stresstest (ramping bis 10 VUs) – misst Verhalten unter Last
 *
 * Endpunkte:
 *   - POST /session              (einmalig im Init-Kontext)
 *   - POST /verify/{session_id}  (unter Last)
 *   - DELETE /session/{id}       (Teardown)
 *
 * Voraussetzungen:
 *   1. Backend läuft
 *   2. cargo run --bin gen_payload → schreibt payload_age.txt, payload_sk.txt
 *
 * Ausführen:
 *   k6 run --env BASE_URL=http://159.195.145.100/age-verification performance_tests.js
 *
 * Mess-Setup:
 *   - Tool:    k6 v2.0.0
 *   - TFHE:    ConfigBuilder::default()
 *   - Datum:   <vor dem Test eintragen>
 *   - Server:  <lokal / Netcup KVM>
 *   - CPU/RAM: <Serverspecs eintragen>
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

//Konfiguration
const BASE_URL      = __ENV.BASE_URL || 'http://159.195.145.100/age-verification';
const ENCRYPTED_AGE = open('./payload_age.txt');
const SERVER_KEY    = open('./payload_sk.txt');

//Session einmalig im Init-Kontext aufbauen
const setupRes = http.post(
    `${BASE_URL}/session`,
    JSON.stringify({ server_key: SERVER_KEY }),
    { headers: { 'Content-Type': 'application/json' }, timeout: '120s' }
);

if (setupRes.status !== 200) {
    throw new Error(`Session-Setup fehlgeschlagen: ${setupRes.status} – ${setupRes.body}`);
}

const SESSION_ID = JSON.parse(setupRes.body).session_id;
console.log(`Session erstellt: ${SESSION_ID}`);

// Payload für alle Requests
const payload = JSON.stringify({ encrypted_age: ENCRYPTED_AGE });

const params = {
    headers: { 'Content-Type': 'application/json' },
    timeout: '120s',
};

//Eigene Metriken
const verifyLatency = new Trend('verify_latency', true);
const errorCount    = new Counter('errors');
const successRate   = new Rate('success_rate');

export const options = {
    scenarios: {
        // Szenario A: Baseline – 1 VU, 10 sequentielle Requests
        baseline: {
            executor: 'per-vu-iterations',
            vus: 1,
            iterations: 10,
            maxDuration: '10m',
            exec: 'verifyFlow',
            tags: { scenario: 'baseline' },
        },

        // Szenario B: Stresstest – ramping bis 10 VUs
        stress: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '30s', target: 2  },
                { duration: '60s', target: 5  },
                { duration: '60s', target: 10 },
                { duration: '30s', target: 0  },
            ],
            gracefulRampDown: '10s',
            exec: 'verifyFlow',
            tags: { scenario: 'stress' },
            startTime: '5m',
        },
    },

    thresholds: {
        'verify_latency': ['p(95)<10000'],
        'success_rate':   ['rate>0.99'],
    },
};

export function verifyFlow() {
    group('verify_age', () => {
        const res = http.post(
            `${BASE_URL}/verify/${SESSION_ID}`,
            payload,
            params
        );

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

    sleep(1);
}

export function teardown() {
    http.del(`${BASE_URL}/session/${SESSION_ID}`);
    console.log(`Session ${SESSION_ID} gelöscht`);
}