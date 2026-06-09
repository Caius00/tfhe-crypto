/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * baseline_load_test.js – Baseline Test (FHE-Grundlatenz je Listenlänge und Bitbreite)
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Misst die reine FHE-Grundlatenz von POST / ohne parallele Last.
 *   1 VU sendet Requests sequentiell – so wird die Mindestlatenz der
 *   homomorphen Statistikberechnung ohne Mutex-Contention bestimmt.
 *
 *   Vier Konfigurationen:
 *     - n=5,  bit_width=8   (Kleinstlast)
 *     - n=10, bit_width=8   (Mittlere Last)
 *     - n=10, bit_width=16  (Höhere Bitbreite)
 *     - n=10, bit_width=32  (Maximale Bitbreite — Trend vollständig)
 *
 * Session-Caching:
 *   setup() lädt den ServerKey einmalig via POST /session hoch.
 *   Alle VUs teilen dieselbe session_id — kein Key-Overhead pro Request mehr.
 *
 * Endpunkte:
 *   - POST /session   (einmalig in setup)
 *   - POST /          (pro Iteration)
 *
 * Voraussetzungen:
 *   1. Backend läuft auf dem Netcup-Server (neuer Build mit Session-API)
 *   2. Payloads generieren (aus src/Load-Tests/):
 *        cargo run --bin gen_payload
 *      → schreibt payload_sk.txt, payload_list_n5_b8.txt,
 *                    payload_list_n10_b8.txt, payload_list_n10_b16.txt,
 *                    payload_list_n10_b32.txt
 *
 * Ausführen:
 *   k6 run \
 *     --env BASE_URL=http://159.195.145.100/statistics \
 *     --out json=results/baseline.json \
 *     services/05-encrypted-statistics-service/src/Load-Tests/baseline_load_test.js
 *
 * Mess-Setup:
 *   - Tool:    k6 v2.0.0
 *   - TFHE:    ConfigBuilder::default()
 *   - Datum:   <vor dem Test eintragen>
 *   - Server:  Netcup KVM
 *   - CPU/RAM: <Serverspecs eintragen>
 *
 * Erwartetes Verhalten:
 *   - Stabile Latenzen ohne Ausreißer bei 1 VU
 *   - Latenz steigt mit n (Median ist O(n log²n))
 *   - Latenz steigt mit Bitbreite
 *   - Deutlich geringerer Traffic als vorher (kein ~80 MB Key mehr pro Request)
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080/statistics';

// Payload-Dateien einmalig im Init-Kontext lesen
const SERVER_KEY   = open('./payload_sk.txt');
const LIST_N5_B8   = JSON.parse(open('./payload_list_n5_b8.txt'));
const LIST_N10_B8  = JSON.parse(open('./payload_list_n10_b8.txt'));
const LIST_N10_B16 = JSON.parse(open('./payload_list_n10_b16.txt'));
const LIST_N10_B32 = JSON.parse(open('./payload_list_n10_b32.txt'));

const params = {
    headers: { 'Content-Type': 'application/json' },
    timeout: '300s',
};

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const latency_n5_b8   = new Trend('latency_n5_b8',   true);
const latency_n10_b8  = new Trend('latency_n10_b8',  true);
const latency_n10_b16 = new Trend('latency_n10_b16', true);
const latency_n10_b32 = new Trend('latency_n10_b32', true);
const errorCount      = new Counter('errors');
const successRate     = new Rate('success_rate');

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    scenarios: {
        baseline: {
            executor: 'per-vu-iterations',
            vus: 1,
            iterations: 5,
            maxDuration: '120m',
            exec: 'baselineFlow',
        },
    },
    thresholds: {
        'latency_n5_b8':   ['p(95)<120000'],
        'latency_n10_b8':  ['p(95)<300000'],
        'latency_n10_b16': ['p(95)<300000'],
        'latency_n10_b32': ['p(95)<600000'],
        'success_rate':    ['rate>0.99'],
    },
};

// ── Setup: ServerKey einmalig hochladen ───────────────────────────────────────
// Läuft einmal vor allen VUs. Rückgabewert wird an jede VU-Funktion übergeben.
export function setup() {
    const res = http.post(
        `${BASE_URL}/session`,
        JSON.stringify({ server_key: SERVER_KEY }),
        params
    );

    if (res.status !== 200) {
        throw new Error(`Session-Erstellung fehlgeschlagen: ${res.status} – ${res.body}`);
    }

    const { session_id } = JSON.parse(res.body);
    console.log(`Session erstellt: ${session_id}`);
    return { session_id };
}

// ── Hilfsfunktion ─────────────────────────────────────────────────────────────
function request(list, bitWidth, trendMetric, label, sessionId) {
    const payload = JSON.stringify({
        session_id: sessionId,
        encrypted_list: list,
        bit_width: bitWidth,
    });

    const res = http.post(`${BASE_URL}/`, payload, params);

    trendMetric.add(res.timings.duration);

    const ok = check(res, {
        [`${label} status 200`]: r => r.status === 200,
        [`${label} has sum`]: r => {
            try { return JSON.parse(r.body).sum !== undefined; } catch { return false; }
        },
        [`${label} has median`]: r => {
            try { return JSON.parse(r.body).median !== undefined; } catch { return false; }
        },
    });

    successRate.add(ok);
    if (!ok) {
        errorCount.add(1);
        console.error(`${label} Fehler: ${res.status} – ${res.body?.slice(0, 200)}`);
    }

    sleep(2);
}

// ── Flow ──────────────────────────────────────────────────────────────────────
// data.session_id kommt von setup() — dieselbe für alle VUs und Iterationen.
export function baselineFlow(data) {
    const sid = data.session_id;
    group('n5_b8',   () => request(LIST_N5_B8,   8,  latency_n5_b8,   'n=5  b=8',  sid));
    group('n10_b8',  () => request(LIST_N10_B8,  8,  latency_n10_b8,  'n=10 b=8',  sid));
    group('n10_b16', () => request(LIST_N10_B16, 16, latency_n10_b16, 'n=10 b=16', sid));
    group('n10_b32', () => request(LIST_N10_B32, 32, latency_n10_b32, 'n=10 b=32', sid));
}
