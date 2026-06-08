/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * stress_results.js – FHE Stress Test (Throughput-Grenze finden)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Isolierter Stress Test des teuersten Endpunkts: GET /results.
 *   Dieser Endpunkt führt die FHE-Auswertung durch (Ciphertexte addieren
 *   mit dem ServerKey) – das ist die rechenintensivste Operation des Systems.
 *
 *   Ziel: Herausfinden ab welcher parallelen Request-Anzahl p95 deutlich
 *   ansteigt, Timeouts auftreten oder der RAM voll läuft.
 *
 * Endpunkte:
 *   - GET /results (isoliert, steigende Last)
 *
 * Voraussetzungen:
 *   1. Backend läuft
 *   2. Session existiert mit mindestens einer abgegebenen Stimme
 *      (sonst gibt /results sofort "ready: false" zurück ohne FHE)
 *   3. session_id und creator_id bekannt
 *
 * Ausführen (lokal):
 *   k6 run --env BASE_URL=http://localhost:8080 \
 *           --env SESSION_ID=<uuid> \
 *           --env CREATOR_ID=<creator-id> \
 *           --out json=results/02_stress_results.json \
 *           services/03-encrypted-voting-polling/load-tests/02_stress_results.js
 *
 * Ausführen (Remote):
 * k6 run --env BASE_URL=http://159.195.145.100/voting --env SESSION_ID=22c98b8b-f0e1-4c5c-b226-a397ec6a75c1 --env CREATOR_ID=kimmy --out json=services/03-encrypted-voting-polling/load-tests/results/stress_results.json services/03-encrypted-voting-polling/load-tests/stress_results.js
 *
 * Mess-Setup:
 *   - Tool:      k6 v2.0.0
 *   - TFHE:      ConfigBuilder::default()
 *   - Datum:     <vor dem Test eintragen>
 *   - Server:    <lokal / Netcup>
 *   - CPU/RAM:   <Serverspecs eintragen>
 *
 * Erwartetes Verhalten:
 *   - Bei 1–2 parallelen VUs: stabile Latenzen
 *   - Ab 3–5 VUs: p95 steigt deutlich (FHE blockiert den Tokio-Thread)
 *   - Ab X VUs: Timeouts oder OOM
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL   = __ENV.BASE_URL   || 'http://159.195.145.100/voting';
const SESSION_ID = __ENV.SESSION_ID || '';
const CREATOR_ID = __ENV.CREATOR_ID || 'kimmy';

if (!SESSION_ID) {
    throw new Error('SESSION_ID fehlt! Bitte --env SESSION_ID=<uuid> angeben.');
}

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const resultsLatency = new Trend('results_latency', true);
const errorCount     = new Counter('errors');
const successRate    = new Rate('success_rate');

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    scenarios: {
        stress_results: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '30s', target: 1  },   // 1 Creator fragt ab
                { duration: '45s', target: 3  },   // 3 Creator fragen gleichzeitig ab
                { duration: '45s', target: 6  },   // 6 parallele Abfragen (CPU-Druck erhöht sich)
                { duration: '45s', target: 10 },   // Peak: 10 parallele FHE-Abfragen
                { duration: '30s', target: 0  },
            ],
            gracefulRampDown: '30s',
            exec: 'fheStressFlow',
        },
    },
    thresholds: {
        'results_latency': ['p(95)<120000'],  // 2 Minuten Limit
        'success_rate':    ['rate>0.95'],
    },
};

export function fheStressFlow() {
    group('fhe_results_request', () => {
        const res = http.get(
            `${BASE_URL}/results/${SESSION_ID}/${CREATOR_ID}`,
            { timeout: '120s' } // FHE dauert, daher hohes Timeout
        );

        resultsLatency.add(res.timings.duration);

        const ok = check(res, {
            'status ist 200': r => r.status === 200,
            'ready ist true': r => {
                try {
                    // Wir prüfen im Body, ob das FHE-Ergebnis wirklich berechnet wurde
                    return JSON.parse(r.body).ready === true;
                } catch (e) {
                    return false;
                }
            },
        });

        successRate.add(ok);

        if (!ok) {
            errorCount.add(1);
            console.error(`Request nicht erfolgreich oder ready=false. Status: ${res.status}`);
        }
    });

    // Pause zwischen den Anfragen der jeweiligen VU
    sleep(5);
}