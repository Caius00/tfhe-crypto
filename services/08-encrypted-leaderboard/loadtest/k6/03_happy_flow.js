/**
 * Happy-Flow: ein Spieler durchläuft 20× den vollständigen Lebenszyklus
 * sequentiell und summiert die Latenz pro Durchlauf.
 *
 * Pro Iteration:
 *   1. GET  /{code}/public-key
 *   2. 5×   POST /{code}/submit  (5 unterschiedliche Player-Keys)
 *   3. GET  /{code}/entries
 *   4. POST /{code}/rank
 *
 * Erfüllt die §3.6-Empfehlung „Happy-Flow über mehrere sequentielle
 * Requests definiert und in Summe gemessen".
 *
 * Self-contained: legt eigenen Raum in setup() an. Custom-Trend
 * `happy_flow_sum_ms` summiert pro Iteration alle 8 Request-Latenzen.
 *
 * Aufruf:
 *   k6 run --out experimental-prometheus-rw 03_happy_flow.js \
 *     -e BASE_URL=http://159.195.145.100/leaderboard
 */

import http from 'k6/http';
import { check, fail, group } from 'k6';
import { Trend } from 'k6/metrics';
import {
  baseUrl,
  createBody,
  submitPayloads,
  testId,
  summaryTrendStats,
  defaultThresholds,
} from './_common.js';

const URL = baseUrl();
const RUN_ID = testId('happy-flow');

const flowSum = new Trend('happy_flow_sum_ms', true);

export const options = {
  vus: 1,
  iterations: 20,
  thresholds: defaultThresholds,
  summaryTrendStats,
  tags: { testid: RUN_ID, scenario: 'happy_flow' },
};

const BODY = createBody();

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
