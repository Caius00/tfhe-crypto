/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * baseline_load_test.js – Age Verification Baseline + Stresstest (session-basiert)
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL      = __ENV.BASE_URL || 'http://159.195.145.100/age-verification';
const ENCRYPTED_AGE = open('./payload_age.txt');
const SERVER_KEY    = open('./payload_sk.txt');

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
        baseline: {
            executor: 'per-vu-iterations',
            vus: 1,
            iterations: 10,
            maxDuration: '10m',
            exec: 'verifyFlow',
            tags: { scenario: 'baseline' },
        },
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

// ── Setup: Session einmalig aufbauen ─────────────────────────────────────────
// setup() läuft einmal vor allen VUs – Rückgabewert wird an verifyFlow übergeben
export function setup() {
    const res = http.post(
        `${BASE_URL}/session`,
        JSON.stringify({ server_key: SERVER_KEY }),
        { headers: { 'Content-Type': 'application/json' }, timeout: '120s' }
    );

    if (res.status !== 200) {
        throw new Error(`Session-Setup fehlgeschlagen: ${res.status} – ${res.body}`);
    }

    const sessionId = JSON.parse(res.body).session_id;
    console.log(`Session erstellt: ${sessionId}`);
    return { sessionId };
}

// ── Flow ──────────────────────────────────────────────────────────────────────
export function verifyFlow(data) {
    const payload = JSON.stringify({ encrypted_age: ENCRYPTED_AGE });

    group('verify_age', () => {
        const res = http.post(
            `${BASE_URL}/verify/${data.sessionId}`,
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

export function teardown(data) {
    http.del(`${BASE_URL}/session/${data.sessionId}`);
    console.log(`Session ${data.sessionId} gelöscht`);
}