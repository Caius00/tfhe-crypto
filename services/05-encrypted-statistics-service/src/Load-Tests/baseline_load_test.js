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
 *   Da die Latenz stark von Listenlänge (n) und Bitbreite abhängt,
 *   werden drei repräsentative Konfigurationen gemessen:
 *     - n=5,  bit_width=8   (Kleinstlast)
 *     - n=10, bit_width=8   (Mittlere Last)
 *     - n=10, bit_width=16  (Höhere Bitbreite)
 *
 * Endpunkte:
 *   - POST /statistics/
 *
 * Voraussetzungen:
 *   1. Backend läuft auf dem Netcup-Server
 *   2. Payloads generieren: cargo run --bin gen_payload
 *      → schreibt payload_sk.txt, payload_list_n5_b8.txt,
 *                    payload_list_n10_b8.txt, payload_list_n10_b16.txt
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
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080/statistics';

// open() im Init-Kontext liest die Datei als String (k6 built-in, kein experimental/fs)
const SERVER_KEY   = open('./payload_sk.txt');
const LIST_N5_B8   = open('./payload_list_n5_b8.txt');
const LIST_N10_B8  = open('./payload_list_n10_b8.txt');
const LIST_N10_B16 = open('./payload_list_n10_b16.txt');

// Payloads einmalig im Init-Kontext bauen – nicht bei jedem Request neu
const payload_n5_b8 = JSON.stringify({
    encrypted_list: JSON.parse(LIST_N5_B8),
    server_key: SERVER_KEY,
    bit_width: 8,
});

const payload_n10_b8 = JSON.stringify({
    encrypted_list: JSON.parse(LIST_N10_B8),
    server_key: SERVER_KEY,
    bit_width: 8,
});

const payload_n10_b16 = JSON.stringify({
    encrypted_list: JSON.parse(LIST_N10_B16),
    server_key: SERVER_KEY,
    bit_width: 16,
});

const params = {
    headers: { 'Content-Type': 'application/json' },
    timeout: '300s',
};

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const latency_n5_b8   = new Trend('latency_n5_b8',   true);
const latency_n10_b8  = new Trend('latency_n10_b8',  true);
const latency_n10_b16 = new Trend('latency_n10_b16', true);
const errorCount      = new Counter('errors');
const successRate     = new Rate('success_rate');

// ── Lastkurve ─────────────────────────────────────────────────────────────────
export const options = {
    scenarios: {
        baseline: {
            executor: 'per-vu-iterations',
            vus: 1,
            iterations: 5,
            maxDuration: '60m',
            exec: 'baselineFlow',
        },
    },
    thresholds: {
        'latency_n5_b8':   ['p(95)<120000'],
        'latency_n10_b8':  ['p(95)<300000'],
        'latency_n10_b16': ['p(95)<300000'],
        'success_rate':    ['rate>0.99'],
    },
};

// ── Hilfsfunktion ─────────────────────────────────────────────────────────────
function request(payload, trendMetric, label) {
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
export function baselineFlow() {
    group('n5_b8',   () => request(payload_n5_b8,   latency_n5_b8,   'n=5  b=8'));
    group('n10_b8',  () => request(payload_n10_b8,  latency_n10_b8,  'n=10 b=8'));
    group('n10_b16', () => request(payload_n10_b16, latency_n10_b16, 'n=10 b=16'));
}