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
const BASE_URL   = __ENV.BASE_URL   || 'http://localhost:8080';
const SESSION_ID = __ENV.SESSION_ID || '';
const CREATOR_ID = __ENV.CREATOR_ID || 'alice';

if (!SESSION_ID) {
    throw new Error('SESSION_ID fehlt! Bitte --env SESSION_ID=<uuid> angeben.');
}

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const resultsLatency = new Trend('results_latency', true);
const errorCount     = new Counter('errors');
const successRate    = new Rate('success_rate');
const timeoutCount   = new Counter('timeouts');

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    scenarios: {
        stress_results: {
            executor: 'ramping-vus',
            startVUs: 1,
            stages: [
                { duration: '30s', target: 1  },   // Baseline
                { duration: '60s', target: 2  },   // Leichte Last
                { duration: '60s', target: 3  },   // Mittlere Last
                { duration: '60s', target: 5  },   // Hohe Last
                { duration: '60s', target: 10 },   // Sehr hohe Last
                { duration: '30s', target: 0  },   // Cool-down
            ],
            gracefulRampDown: '30s',
        },
    },

    thresholds: {
        // Absichtlich hoch gesetzt – wir wollen sehen wo es kippt
        'results_latency': ['p(95)<120000'],  // 2 Minuten Timeout
        'success_rate':    ['rate>0.95'],     // max 5% Fehler erlaubt
    },
};

// ── Haupt-Flow ────────────────────────────────────────────────────────────────
export default function () {
    group('results', () => {
        const res = http.get(
            `${BASE_URL}/results/${SESSION_ID}/${CREATOR_ID}`,
            { timeout: '120s' }
        );

        resultsLatency.add(res.timings.duration);

        if (res.status === 0) {
            // Timeout
            timeoutCount.add(1);
            errorCount.add(1);
            successRate.add(false);
            console.error(`Timeout nach ${res.timings.duration}ms`);
            return;
        }

        const ok = check(res, {
            'results 200': r => r.status === 200,
            'results hat body': r => r.body && r.body.length > 0,
        });

        successRate.add(ok);
        if (!ok) {
            errorCount.add(1);
            console.error(`Results Fehler: ${res.status} – ${res.body?.substring(0, 200)}`);
        } else {
            try {
                const body = JSON.parse(res.body);
                if (!body.ready) {
                    console.warn('Results noch nicht bereit (keine Stimmen?)');
                }
            } catch (e) {
                console.warn('Results Body Parse Fehler');
            }
        }
    });

    // Pause zwischen Requests – FHE braucht Zeit
    sleep(3);
}