/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * normal_flow.js – Normalbetrieb Last Test
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Simuliert den realistischen Normalbetrieb einer Voting-Session mit
 *   gleichzeitigen Teilnehmern. Testet den kompletten Lifecycle:
 *   Join → Status-Polling → Pending abrufen → Approve → Results abrufen.
 *
 * Endpunkte:
 *   - POST /join              (viele parallele Teilnehmer)
 *   - GET  /status            (jeder Teilnehmer pollt alle 2s)
 *   - GET  /pending           (Creator pollt regelmäßig)
 *   - POST /approve           (Creator genehmigt Teilnehmer)
 *   - GET  /results           (Creator ruft Ergebnisse ab)
 *
 * Nicht getestet (begründet):
 *   - POST /session  → einmalig, kein Lastproblem
 *   - POST /vote     → FHE-Ciphertexte zu groß für k6-Parameter
 *   - GET  /session  → nur Metadaten, kein FHE
 *   - POST /finalize → einmalig pro Session
 *
 * Voraussetzungen:
 *   1. Backend läuft (lokal oder Remote)
 *   2. Session im Frontend erstellen (Keys generieren + Session anlegen)
 *   3. Mindestens einen Teilnehmer manuell genehmigen und abstimmen lassen
 *   4. session_id aus der URL kopieren (/voting/manage/<session_id>)
 *
 * Ausführen (lokal):
 *   k6 run --env BASE_URL=http://localhost:8080 \
 *           --env SESSION_ID=<uuid> \
 *           --env CREATOR_ID=<creator-id> \
 *           --out json=results/01_normal_flow.json \
 *           services/03-encrypted-voting-polling/load-tests/01_normal_flow.js
 *
 * Ausführen (Remote):
 * k6 run --env BASE_URL=http://159.195.145.100/voting --env SESSION_ID=<uuid> --env CREATOR_ID=<creator-id> --out json=services/03-encrypted-voting-polling/load-tests/results/normal_flow.json services/03-encrypted-voting-polling/load-tests/normal_flow.js
 *
 * Mess-Setup:
 *   - Tool:      k6 v2.0.0
 *   - TFHE:      ConfigBuilder::default()
 *   - Datum:     <vor dem Test eintragen>
 *   - Server:    <lokal / Netcup>
 *   - CPU/RAM:   <Serverspecs eintragen>
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL   = __ENV.BASE_URL   || 'http://localhost:8080';
const SESSION_ID = __ENV.SESSION_ID || '';
const CREATOR_ID = __ENV.CREATOR_ID || 'alice';

if (!SESSION_ID) {
    throw new Error('SESSION_ID fehlt! Bitte --env SESSION_ID=<uuid> angeben.');
}

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const joinLatency    = new Trend('join_latency',    true);
const statusLatency  = new Trend('status_latency',  true);
const pendingLatency = new Trend('pending_latency', true);
const approveLatency = new Trend('approve_latency', true);
const resultsLatency = new Trend('results_latency', true);
const errorCount     = new Counter('errors');
const successRate    = new Rate('success_rate');

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    scenarios: {

        // Szenario A: Join + Status-Polling (viele parallele Teilnehmer)
        join_and_status: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '20s', target: 5  },   // Warm-up
                { duration: '60s', target: 10 },   // Normallast
                { duration: '60s', target: 25 },   // Erhöhte Last
                { duration: '60s', target: 50 },   // Stresslast
                { duration: '20s', target: 0  },   // Cool-down
            ],
            gracefulRampDown: '10s',
            exec: 'joinAndStatusFlow',
        },

        // Szenario B: Creator pollt Pending-Liste + genehmigt Teilnehmer
        creator_flow: {
            executor: 'constant-vus',
            vus: 2,
            duration: '3m',
            exec: 'creatorFlow',
            startTime: '10s',
        },

        // Szenario C: Ergebnisse abrufen (FHE – teuerster Endpunkt)
        results_poll: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '30s', target: 1 },
                { duration: '60s', target: 2 },
                { duration: '60s', target: 3 },
                { duration: '30s', target: 0 },
            ],
            gracefulRampDown: '10s',
            exec: 'resultsFlow',
            startTime: '20s',
        },
    },

    thresholds: {
        'join_latency':    ['p(95)<500'],    // Join unter 500ms
        'status_latency':  ['p(95)<200'],    // Status-Polling unter 200ms
        'pending_latency': ['p(95)<300'],    // Pending unter 300ms
        'approve_latency': ['p(95)<400'],    // Approve unter 400ms
        'results_latency': ['p(95)<60000'],  // FHE-Auswertung unter 60s
        'success_rate':    ['rate>0.99'],    // Fehlerrate unter 1%
    },
};

// ── Szenario A: Join + Status-Polling ────────────────────────────────────────
export function joinAndStatusFlow() {
    const participantId = `p-${Math.random().toString(36).slice(2, 10)}`;

    group('join', () => {
        const res = http.post(
            `${BASE_URL}/join`,
            JSON.stringify({
                session_id: SESSION_ID,
                participant_id: participantId,
                enc_name_chunks: null,
            }),
            { headers: { 'Content-Type': 'application/json' } }
        );

        joinLatency.add(res.timings.duration);
        const ok = check(res, {
            'join 200': r => r.status === 200,
            'join pending': r => {
                try { return JSON.parse(r.body).status === 'pending'; } catch { return false; }
            },
        });
        successRate.add(ok);
        if (!ok) {
            errorCount.add(1);
            console.error(`Join Fehler: ${res.status} – ${res.body}`);
        }
    });

    sleep(0.5);

    group('status', () => {
        const res = http.get(`${BASE_URL}/status/${SESSION_ID}/${participantId}`);

        statusLatency.add(res.timings.duration);
        const ok = check(res, { 'status 200': r => r.status === 200 });
        successRate.add(ok);
        if (!ok) errorCount.add(1);
    });

    sleep(1);
}

// ── Szenario B: Creator Flow (Pending + Approve) ──────────────────────────────
export function creatorFlow() {

    // Pending-Liste abrufen
    group('pending', () => {
        const res = http.get(`${BASE_URL}/pending/${SESSION_ID}/${CREATOR_ID}`);

        pendingLatency.add(res.timings.duration);
        const ok = check(res, { 'pending 200': r => r.status === 200 });
        successRate.add(ok);
        if (!ok) errorCount.add(1);

        // Falls Teilnehmer pending sind → einen genehmigen
        try {
            const body = JSON.parse(res.body);
            if (Array.isArray(body) && body.length > 0) {
                const participantId = body[0].participant_id;

                group('approve', () => {
                    const approveRes = http.post(
                        `${BASE_URL}/approve`,
                        JSON.stringify({
                            session_id: SESSION_ID,
                            creator_id: CREATOR_ID,
                            participant_id: participantId,
                            approved: true,
                        }),
                        { headers: { 'Content-Type': 'application/json' } }
                    );

                    approveLatency.add(approveRes.timings.duration);
                    const approveOk = check(approveRes, { 'approve 200': r => r.status === 200 });
                    successRate.add(approveOk);
                    if (!approveOk) errorCount.add(1);
                });
            }
        } catch (e) {
            console.warn('Pending-Parse Fehler:', e);
        }
    });

    sleep(2);
}

// ── Szenario C: Ergebnisse abrufen (FHE) ─────────────────────────────────────
export function resultsFlow() {
    group('results', () => {
        const res = http.get(
            `${BASE_URL}/results/${SESSION_ID}/${CREATOR_ID}`,
            { timeout: '120s' }
        );

        resultsLatency.add(res.timings.duration);
        const ok = check(res, { 'results 200': r => r.status === 200 });
        successRate.add(ok);
        if (!ok) {
            errorCount.add(1);
            console.error(`Results Fehler: ${res.status} – ${res.body}`);
        }
    });

    sleep(5);
}