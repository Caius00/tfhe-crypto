/**
 * Session-Crash-Test: legt alle 30 s eine neue Session an, ohne je eine zu
 * schließen. Jede Session hält den dekomprimierten ServerKey im RAM
 * (mehrere hundert MB), daher wächst der Pod-Speicher linear.
 *
 * Beantwortet die Spec-Frage „läuft der RAM voll?" (§3.6) mit einer harten
 * Zahl: `max_sessions = letzte erfolgreiche Iteration vor erstem Fehler`.
 *
 * Sicherheits-Mechanismen:
 * - Abort bei Failure-Rate > 5 % nach den ersten 30 s (`abortOnFailure`).
 * - Maximale Laufzeit 30 min — danach manuell verlängern, falls der Pod
 *   weiter standhält.
 *
 * WICHTIG während des Runs: Grafana-Dashboard offen halten und auf
 * `node_memory_MemAvailable_bytes` schauen — bei <500 MiB Ctrl+C, damit
 * der k3s-Knoten nicht in OOM-Killer-Spiralen läuft.
 *
 * Aufruf:
 *   k6 run --out experimental-prometheus-rw 02_session_crash.js \
 *     -e BASE_URL=http://159.195.145.100/leaderboard
 */

import http from 'k6/http';
import { check } from 'k6';
import {
  baseUrl,
  createBody,
  testId,
  summaryTrendStats,
  defaultThresholds,
  abortOnFailure,
} from './_common.js';

const RUN_ID = testId('session-crash');
const URL = baseUrl();
const BODY = createBody();

export const options = {
  scenarios: {
    create_storm: {
      executor: 'constant-arrival-rate',
      rate: 1,
      timeUnit: '30s',
      duration: '30m',
      // preAllocatedVUs muss zur erwarteten Concurrency passen — eine Session
      // anzulegen dauert mehrere Sekunden, also brauchen wir mindestens 4 VUs
      // damit sich Anlage-Phasen überlappen können wenn der Pod langsamer wird.
      preAllocatedVUs: 4,
      maxVUs: 8,
    },
  },
  thresholds: {
    ...defaultThresholds,
    ...abortOnFailure,
  },
  summaryTrendStats,
  tags: { testid: RUN_ID, scenario: 'session_crash' },
};

export default function () {
  const res = http.post(`${URL}/create`, BODY, {
    headers: { 'Content-Type': 'application/json' },
    tags: { endpoint: 'create' },
    timeout: '120s',
  });

  check(res, {
    'create 200': (r) => r.status === 200,
  });
}
