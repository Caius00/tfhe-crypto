/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * spike_join.js – Spike Test (plötzlicher Teilnehmer-Ansturm)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Simuliert einen plötzlichen Ansturm vieler Teilnehmer die gleichzeitig
 *   einer Session beitreten – z.B. wenn ein Link geteilt wird und viele
 *   Nutzer gleichzeitig klicken.
 *
 *   Im Gegensatz zu 01_normal_flow.js gibt es hier KEINE schrittweise
 *   Erhöhung – die Last springt sofort von 0 auf Maximum.
 *
 *   Testet ob der Mutex-Lock auf AppState unter hoher Last zu Starvation
 *   führt und wie schnell sich das System nach dem Spike erholt.
 *
 * Endpunkte:
 *   - POST /join   (Spike: 0 → 100 VUs sofort)
 *   - GET  /status (Folgelast: alle beigetretenen Teilnehmer pollen)
 *
 * Voraussetzungen:
 *   1. Backend läuft
 *   2. Offene (nicht finalisierte) Session existiert
 *   3. session_id bekannt
 *
 * Ausführen (lokal):
 *   k6 run --env BASE_URL=http://localhost:8080 \
 *           --env SESSION_ID=<uuid> \
 *           --out json=results/spike_join.json \
 *           services/03-encrypted-voting-polling/load-tests/spike_join.js
 *
 * Ausführen (Remote):
 * k6 run --env BASE_URL=http://159.195.145.100/voting --env SESSION_ID=81aa96de-3b05-42e9-bab3-527a61239774 --out json=results/spike_join.json services/03-encrypted-voting-polling/load-tests/spike_join.js
 *
 * Mess-Setup:
 *   - Tool:      k6 v2.0.0
 *   - TFHE:      ConfigBuilder::default()
 *   - Datum:     <vor dem Test eintragen>
 *   - Server:    <lokal / Netcup>
 *   - CPU/RAM:   <Serverspecs eintragen>
 *
 * Erwartetes Verhalten:
 *   - Spike-Phase: p95 steigt kurz an (Mutex-Contention)
 *   - Recovery-Phase: p95 fällt wieder auf Baseline
 *   - Kritisch: Fehlerrate während Spike unter 1% halten
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL   = __ENV.BASE_URL   || 'http://159.195.145.100/voting';
const SESSION_ID = __ENV.SESSION_ID || '';

if (!SESSION_ID) {
    throw new Error('SESSION_ID fehlt! Bitte --env SESSION_ID=<uuid> angeben.');
}

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const joinLatency   = new Trend('join_latency',   true);
const statusLatency = new Trend('status_latency', true);
const errorCount    = new Counter('errors');
const successRate   = new Rate('success_rate');

// ── Lastkurve: Spike ──────────────────────────────────────────────────────────
export const options = {
    scenarios: {
        spike: {
            executor: 'ramping-vus',
            startVUs: 0,
            stages: [
                { duration: '10s', target: 5   },  // Baseline
                { duration: '5s',  target: 100 },  // ← Spike: sofort 100 VUs
                { duration: '30s', target: 100 },  // Spike halten
                { duration: '10s', target: 5   },  // Recovery
                { duration: '30s', target: 5   },  // Stabilität nach Recovery
                { duration: '10s', target: 0   },  // Cool-down
            ],
            gracefulRampDown: '10s',
            exec: 'spikeFlow',
        },
    },

    thresholds: {
        'join_latency':   ['p(95)<2000'],  // Während Spike max 2s
        'status_latency': ['p(95)<500'],   // Status max 500ms
        'success_rate':   ['rate>0.95'],   // max 5% Fehler während Spike
    },
};

// ── Haupt-Flow ────────────────────────────────────────────────────────────────
export function spikeFlow() {
    const participantId = `p-${Math.random().toString(36).slice(2, 10)}`;

    group('join_spike', () => {
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
            console.error(`Join Fehler: ${res.status}`);
        }
    });

    sleep(0.2);

    group('status_spike', () => {
        const res = http.get(`${BASE_URL}/status/${SESSION_ID}/${participantId}`);

        statusLatency.add(res.timings.duration);
        const ok = check(res, { 'status 200': r => r.status === 200 });
        successRate.add(ok);
        if (!ok) errorCount.add(1);
    });

    sleep(0.5);
}