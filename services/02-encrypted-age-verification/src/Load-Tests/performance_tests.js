/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * performance_tests.js – Age Verification Last- und Stresstest
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Der einzige Endpunkt POST / wird unter zwei Szenarien gemessen:
 *   - Szenario A: Baseline (1 VU, sequentiell) – misst Grundlatenz der FHE-Operation
 *   - Szenario B: Stresstest (ramping bis 10 VUs) – misst Mutex-Verhalten unter Last
 *
 * Endpunkte:
 *   - POST /age-verification/   (FHE-Altersverifikation)
 *
 * Nicht getestet (begründet):
 *   - GET /health  → kein FHE, kein Lastproblem
 *   - GET /docs    → statisch
 *
 * Voraussetzungen:
 *   1. Backend läuft auf dem Netcup-Server
 *   2. Payload generieren: cargo run --bin gen_payload
 *   3. ENCRYPTED_AGE und SERVER_KEY als Umgebungsvariablen setzen (s.u.)
 *
 * Ausführen:
 * Mess-Setup:
 *   - Tool:    k6 v2.0.0
 *   - TFHE:    ConfigBuilder::default()
 *   - Datum:   <vor dem Test eintragen>
 *   - Server:  Netcup KVM
 *   - CPU/RAM: <Serverspecs eintragen>
 * ═══════════════════════════════════════════════════════════════════════════════
 */
 
import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';
 
// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL      = __ENV.BASE_URL      || 'http://159.195.145.100/age-verification';
const ENCRYPTED_AGE = open('./payload_age.txt');
const SERVER_KEY    = open('./payload_sk.txt');
 
if (!ENCRYPTED_AGE || !SERVER_KEY) {
    throw new Error(
        'ENCRYPTED_AGE und SERVER_KEY fehlen!\n' +
        'Payload generieren mit: cargo run --bin gen_payload\n' +
        'Dann: --env ENCRYPTED_AGE=<base64> --env SERVER_KEY=<base64>'
    );
}
 
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
 
        // Szenario A: Baseline – 1 VU, 10 sequentielle Requests
        // Misst die reine FHE-Grundlatenz ohne Mutex-Contention
        baseline: {
            executor: 'per-vu-iterations',
            vus: 1,
            iterations: 10,
            maxDuration: '10m',
            exec: 'verifyFlow',
            tags: { scenario: 'baseline' },
        },
 
        // Szenario B: Stresstest – ramping bis 10 VUs
        // Zeigt ab wann der Mutex den Durchsatz drosselt und p95 ansteigt
        stress: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '30s', target: 2  },   // Warm-up
                { duration: '60s', target: 5  },   // Mittlere Last
                { duration: '60s', target: 10 },   // Stresslast
                { duration: '30s', target: 0  },   // Cool-down
            ],
            gracefulRampDown: '10s',
            exec: 'verifyFlow',
            tags: { scenario: 'stress' },
            startTime: '5m',   // Nach Baseline starten
        },
    },
 
    thresholds: {
        'verify_latency': ['p(95)<60000'],  // FHE-Auswertung unter 60s
        'success_rate':   ['rate>0.99'],    // Fehlerrate unter 1%
    },
};
 
// ── Haupt-Flow ────────────────────────────────────────────────────────────────
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
 
    sleep(1);
}