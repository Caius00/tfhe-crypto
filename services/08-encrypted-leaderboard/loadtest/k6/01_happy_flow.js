/**
 * Happy-Flow: simuliert einen einzelnen Spieler-Lebenszyklus über mehrere
 * sequentielle Requests und summiert die Latenz pro Durchlauf.
 *
 * Pro Iteration:
 *   1. GET  /{code}/public-key
 *   2. 5×   POST /{code}/submit   (verschiedene Player-Keys, runde-pro-Iteration)
 *   3. GET  /{code}/entries
 *   4. POST /{code}/rank
 *
 * Erfüllt die Spec-Anforderung „Happy-Flow über mehrere sequentielle Requests
 * definieren und in Summe messen" (§3.6).
 *
 * Selbst-startend: legt in `setup()` einen eigenen Raum an — kein Vorab-Setup nötig.
 *
 * Aufruf:
 *   k6 run --out experimental-prometheus-rw 01_happy_flow.js \
 *     -e BASE_URL=http://159.195.145.100/leaderboard
 */

import http from 'k6/http';
import { check, fail, group, Trend } from 'k6';
import {
  baseUrl,
  createBody,
  submitPayloads,
  testId,
  summaryTrendStats,
  defaultThresholds,
} from './_common.js';

const RUN_ID = testId('happy-flow');
const URL = baseUrl();

// Eigener Trend für die Summe pro Iteration — k6's `iteration_duration`
// enthält auch Sleep/Setup-Anteile, daher messen wir den reinen Request-Pfad selbst.
const flowSum = new Trend('happy_flow_sum_ms', true);

export const options = {
  vus: 1,
  iterations: 20,
  thresholds: defaultThresholds,
  summaryTrendStats,
  tags: { testid: RUN_ID, scenario: 'happy_flow' },
};

const BODY = createBody();

/** Legt einen frischen Raum an, gibt den Code an alle VUs weiter. */
export function setup() {
  const r = http.post(`${URL}/create`, BODY, {
    headers: { 'Content-Type': 'application/json' },
    tags: { endpoint: 'create', phase: 'setup' },
    timeout: '120s',
  });
  if (r.status !== 200) {
    fail(`Setup-Raum konnte nicht angelegt werden: status=${r.status} body=${r.body}`);
  }
  const code = r.json('code');
  console.log(`__LEADERBOARD_ROOM_CODE__=${code}`);
  return { code };
}

export default function (data) {
  const CODE = data.code;
  const t0 = Date.now();

  group('public-key', () => {
    const r = http.get(`${URL}/${CODE}/public-key`, {
      tags: { endpoint: 'public_key' },
    });
    check(r, { 'public-key 200': (x) => x.status === 200 });
  });

  group('submit-5x', () => {
    // 5 unterschiedliche Player-Keys pro Runde — über `__ITER` rotieren wir,
    // damit nicht alle Iterationen exakt denselben Spieler ansprechen.
    for (let i = 0; i < 5; i++) {
      const payloadIdx = (__ITER * 5 + i) % submitPayloads.length;
      const p = submitPayloads[payloadIdx];
      const r = http.post(`${URL}/${CODE}/submit`, JSON.stringify(p), {
        headers: { 'Content-Type': 'application/json' },
        tags: { endpoint: 'submit' },
        timeout: '60s',
      });
      check(r, { 'submit 200': (x) => x.status === 200 });
    }
  });

  group('entries', () => {
    const r = http.get(`${URL}/${CODE}/entries`, {
      tags: { endpoint: 'entries' },
    });
    check(r, { 'entries 200': (x) => x.status === 200 });
  });

  group('rank', () => {
    // Wir nutzen ein beliebiges `encrypted_id` aus dem Corpus als Such-Target.
    const target = submitPayloads[__ITER % submitPayloads.length].encrypted_id;
    const r = http.post(
      `${URL}/${CODE}/rank`,
      JSON.stringify({ encrypted_id: target }),
      {
        headers: { 'Content-Type': 'application/json' },
        tags: { endpoint: 'rank' },
        timeout: '60s',
      },
    );
    check(r, { 'rank 200': (x) => x.status === 200 });
  });

  flowSum.add(Date.now() - t0);
}
