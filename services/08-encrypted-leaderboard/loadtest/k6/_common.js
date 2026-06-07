/**
 * Geteilte Helfer für alle Leaderboard-Loadtest-Skripte.
 *
 * Hält alles zentral, was sonst in jedem Skript doppelt stünde:
 * - URL- und Code-Auflösung aus Environment-Variablen
 * - Lesen der vorab vom Rust-Tool generierten Corpus-Dateien
 * - Default-Options (Tags, Trend-Stats, Thresholds)
 *
 * Kein Skript fasst `__ENV` oder `open()` direkt an — alles geht über diese
 * Datei, damit Pfade und Defaults nur an einer Stelle gepflegt werden.
 */

import { SharedArray } from 'k6/data';

// ---------------------------------------------------------------------------
// Pfade & Umgebung
// ---------------------------------------------------------------------------

/**
 * Basis-URL des Services. Wird per `-e BASE_URL=...` übergeben.
 * Default zeigt auf den lokalen Service-Port — sinnvoll wenn man mal direkt
 * gegen einen `cargo run` testen will, statt gegen Prod.
 */
export function baseUrl() {
  return __ENV.BASE_URL || 'http://localhost:8080';
}

/**
 * Lädt den `POST /create`-Request-Body aus dem Corpus. Liefert den fertigen
 * JSON-String inkl. Server-Key (mehrere hundert MB) — wird nur einmal pro VU
 * geladen und sollte daher nicht in der Hot-Loop stehen.
 *
 * Alle Tests legen ihren Raum selbst in ihrer `setup()`-Phase an, indem sie
 * diesen Body an `POST /create` schicken.
 */
export function createBody() {
  return open('../corpus/create_body.json');
}

/**
 * Submit-Payloads aus dem NDJSON-Corpus. Liefert ein `SharedArray`, das k6
 * prozessweit teilt — verhindert dass jede VU ihre eigene Kopie im Heap hat
 * (sonst würden 200 Payloads × N VUs den RAM unnötig sprengen).
 */
export const submitPayloads = new SharedArray('submit_payloads', () => {
  const raw = open('../corpus/submit_payloads.ndjson');
  return raw
    .split('\n')
    .filter((l) => l.length > 0)
    .map((l) => JSON.parse(l));
});

// ---------------------------------------------------------------------------
// Defaults für k6-Options
// ---------------------------------------------------------------------------

/**
 * Eindeutige Test-Run-ID. Wandert als Tag in jede Metrik und wird im Grafana-
 * Dashboard als `$testid`-Variable benutzt, damit unterschiedliche Runs
 * sauber getrennt darstellbar sind.
 * Format: `<scenario>-YYYY-MM-DD-HHMM` (UTC), z.B. `submit-sweep-2026-05-29-1430`.
 */
export function testId(scenario) {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, '0');
  const stamp =
    now.getUTCFullYear() +
    '-' +
    pad(now.getUTCMonth() + 1) +
    '-' +
    pad(now.getUTCDate()) +
    '-' +
    pad(now.getUTCHours()) +
    pad(now.getUTCMinutes());
  return __ENV.TESTID || `${scenario}-${stamp}`;
}

/**
 * Konsistente Tail-Statistiken im k6-Summary. p(95) und p(99) sind die
 * Werte, die in der Spec-Sektion §3.6 verlangt werden.
 */
export const summaryTrendStats = ['min', 'med', 'p(95)', 'p(99)', 'max', 'count'];

/**
 * Default-Threshold: jede HTTP-Antwort soll innerhalb von 10 s zurückkommen.
 * Wert kommt aus der Spec — alles darüber gilt als Timeout-Verdacht.
 * Wird in einzelnen Skripten überschrieben/verschärft.
 */
export const defaultThresholds = {
  http_req_duration: ['p(95)<10000'],
};

/**
 * Threshold mit Abort-Verhalten — für Tests, die das System absichtlich
 * an die Grenze fahren (Session-Crash). Bricht ab, sobald >5% der Requests
 * fehlschlagen, damit der Pod nicht in einen unerholbaren Zustand kippt.
 */
export const abortOnFailure = {
  http_req_failed: [
    {
      threshold: 'rate<0.05',
      abortOnFail: true,
      delayAbortEval: '30s',
    },
  ],
};
