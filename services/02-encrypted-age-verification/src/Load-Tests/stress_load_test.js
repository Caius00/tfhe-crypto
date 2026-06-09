/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * stress_load_test.js – FHE Stress Test (realistisch, pro-VU Schlüsselpaar)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Jede VU simuliert einen eigenen Client mit eigenem Schlüsselpaar:
 *   - Eigener ServerKey (payload_vu{N}_sk.txt)
 *   - Eigenes encrypted_age (payload_vu{N}_age.txt)
 *   - Eigene Session per POST /session
 *   - Verify per POST /verify/{session_id}
 *
 * Voraussetzungen:
 *   1. Backend läuft
 *   2. cargo run --bin gen_payload
 *      → schreibt payload_vu1_sk.txt bis payload_vu10_sk.txt
 *                 payload_vu1_age.txt bis payload_vu10_age.txt
 *
 * Ausführen:
 *   k6 run stress_load_test.js
 *
 * Mess-Setup:
 *   - Tool:    k6 v2.0.0
 *   - TFHE:    ConfigBuilder::default()
 *   - Datum:   <vor dem Test eintragen>
 *   - Server:  <lokal / Netcup KVM>
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL   = __ENV.BASE_URL || 'http://159.195.145.100/age-verification';
const MAX_VUS    = 10;

// Alle Payloads im Init-Kontext laden
const SERVER_KEYS    = [];
const ENCRYPTED_AGES = [];

for (let i = 1; i <= MAX_VUS; i++) {
    SERVER_KEYS.push(open(`./payload_vu${i}_sk.txt`));
    ENCRYPTED_AGES.push(open(`./payload_vu${i}_age.txt`));
}

const params = {
    headers: { 'Content-Type': 'application/json' },
    timeout: '120s',
};

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const verifyLatency = new Trend('verify_latency', true);
const setupLatency  = new Trend('setup_latency',  true);
const errorCount    = new Counter('errors');
const successRate   = new Rate('success_rate');

// ── Pro-VU State ──────────────────────────────────────────────────────────────
const vuSessions = {};

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    scenarios: {
        stress: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '30s', target: 1  },
                { duration: '60s', target: 3  },
                { duration: '60s', target: 6  },
                { duration: '60s', target: 10 },
                { duration: '30s', target: 0  },
            ],
            gracefulRampDown: '30s',
            exec: 'verifyFlow',
        },
    },
    thresholds: {
        'verify_latency': ['p(95)<10000'],
        'setup_latency':  ['p(95)<120000'],
        'success_rate':   ['rate>0.99'],
    },
};

// ── Flow ──────────────────────────────────────────────────────────────────────
export function verifyFlow() {
    // VU-Index: 1-basiert, max MAX_VUS (wraparound falls mehr VUs als Paare)
    const vuIndex = ((__VU - 1) % MAX_VUS);
    const serverKey    = SERVER_KEYS[vuIndex];
    const encryptedAge = ENCRYPTED_AGES[vuIndex];

    // Erste Iteration dieser VU: eigene Session aufbauen
    if (!vuSessions[__VU]) {
        group('session_setup', () => {
            const res = http.post(
                `${BASE_URL}/session`,
                JSON.stringify({ server_key: serverKey }),
                { headers: { 'Content-Type': 'application/json' }, timeout: '120s' }
            );

            setupLatency.add(res.timings.duration);

            if (res.status !== 200) {
                errorCount.add(1);
                console.error(`VU ${__VU} Session-Setup fehlgeschlagen: ${res.status}`);
                return;
            }

            vuSessions[__VU] = JSON.parse(res.body).session_id;
            console.log(`VU ${__VU} Session erstellt: ${vuSessions[__VU]}`);
        });
    }

    if (!vuSessions[__VU]) return;

    // Verify mit eigenem encrypted_age
    group('verify_age', () => {
        const payload = JSON.stringify({ encrypted_age: encryptedAge });
        const res = http.post(
            `${BASE_URL}/verify/${vuSessions[__VU]}`,
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
            console.error(`VU ${__VU} Fehler: ${res.status} – ${res.body}`);
        }
    });

    sleep(1);
}

// ── Teardown ──────────────────────────────────────────────────────────────────
export function teardown() {
    for (const [vu, sessionId] of Object.entries(vuSessions)) {
        http.del(`${BASE_URL}/session/${sessionId}`);
        console.log(`VU ${vu} Session ${sessionId} gelöscht`);
    }
}