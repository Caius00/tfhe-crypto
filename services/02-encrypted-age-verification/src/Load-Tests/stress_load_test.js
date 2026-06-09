/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * stress_load_test.js – FHE Stress Test (10 parallele Clients, pre-built Sessions)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   setup() baut alle 10 Sessions sequentiell auf bevor der Test startet.
 *   Danach greifen alle VUs parallel auf ihre eigene Session zu – der
 *   ServerKey-Upload beeinflusst die Verify-Latenz nicht mehr.
 *
 * Endpunkte:
 *   - POST /session             (10x in setup(), vor dem Test)
 *   - POST /verify/{session_id} (unter Last, jede VU eigene Session)
 *   - DELETE /session/{id}      (10x in teardown())
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
const BASE_URL  = __ENV.BASE_URL || 'http://159.195.145.100/age-verification';
const MAX_VUS   = 10;

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
const errorCount    = new Counter('errors');
const successRate   = new Rate('success_rate');

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    setupTimeout: '300s',  // 10 Sessions à ~15-30s = bis zu 300s
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
        'success_rate':   ['rate>0.99'],
    },
};

// ── Setup: alle Sessions vorab aufbauen ───────────────────────────────────────
// Läuft einmal vor allen VUs – gibt Array von session_ids zurück
export function setup() {
    const sessionIds = [];

    for (let i = 0; i < MAX_VUS; i++) {
        console.log(`Setup Session ${i + 1}/${MAX_VUS}...`);
        const res = http.post(
            `${BASE_URL}/session`,
            JSON.stringify({ server_key: SERVER_KEYS[i] }),
            { headers: { 'Content-Type': 'application/json' }, timeout: '120s' }
        );

        if (res.status !== 200) {
            throw new Error(`Session ${i + 1} Setup fehlgeschlagen: ${res.status} – ${res.body}`);
        }

        const sessionId = JSON.parse(res.body).session_id;
        sessionIds.push(sessionId);
        console.log(`Session ${i + 1} erstellt: ${sessionId}`);
    }

    console.log(`Alle ${MAX_VUS} Sessions bereit. Stresstest beginnt.`);
    return { sessionIds };
}

// ── Flow ──────────────────────────────────────────────────────────────────────
export function verifyFlow(data) {
    // VU-Index: 0-basiert, wraparound falls mehr VUs als Sessions
    const vuIndex  = (__VU - 1) % MAX_VUS;
    const sessionId    = data.sessionIds[vuIndex];
    const encryptedAge = ENCRYPTED_AGES[vuIndex];

    group('verify_age', () => {
        const payload = JSON.stringify({ encrypted_age: encryptedAge });
        const res = http.post(
            `${BASE_URL}/verify/${sessionId}`,
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
export function teardown(data) {
    for (let i = 0; i < data.sessionIds.length; i++) {
        http.del(`${BASE_URL}/session/${data.sessionIds[i]}`);
        console.log(`Session ${i + 1} gelöscht: ${data.sessionIds[i]}`);
    }
}