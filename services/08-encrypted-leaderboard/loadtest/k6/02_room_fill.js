/**
 * Room-Fill: 1 Raum, 20 Spieler submitten SEQUENTIELL je einen Wert
 * (Spieler 2 wartet bis Spieler 1 fertig ist usw.).
 *
 * Genau die Situation aus `MAX_ENTRIES = 20` (siehe `state.rs:25`): wir
 * füllen einen Raum bis zur Kapazitätsgrenze.
 *
 * Pro Iteration `i ∈ [0..19]`:
 *   1. `POST /submit` mit player_key=`player_i`
 *   2. Kurz schlafen, damit der Hintergrund-Sort einen Pass machen kann
 *      bevor der nächste Spieler einreicht
 *
 * Was du im Dashboard siehst:
 *   - Submit-Latenz konstant (immer eine einzelne Insertion, kein FHE keep_max)
 *   - Pod-CPU steigt mit jeder Iteration, weil der Sort immer mehr Elemente
 *     vergleichen muss (n=1 → n=2 → … → n=20)
 *   - Submit p95 sollte konstant bleiben; die echte Skalierungs-Aussage
 *     bekommst du aus der CPU-Last und dem Vergleich zur Sort-Komplexität
 *     (Batcher's Odd-Even Merge ~ O(n · log²n))
 *
 * Aufruf:
 *   k6 run --out experimental-prometheus-rw 02_room_fill.js \
 *     -e BASE_URL=http://159.195.145.100/leaderboard
 */

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import {
  baseUrl,
  createBody,
  submitPayloads,
  testId,
  summaryTrendStats,
  defaultThresholds,
} from './_common.js';

const URL = baseUrl();
const RUN_ID = testId('room-fill');
const SETTLE_SEC = parseFloat(__ENV.SETTLE_SEC || '5');

export const options = {
  scenarios: {
    fill: {
      executor: 'per-vu-iterations',
      vus: 1,
      iterations: 20,
      // Großzügig: 20 × (submit + sort) kann je nach Hardware mehrere Minuten dauern
      maxDuration: '15m',
    },
  },
  thresholds: defaultThresholds,
  summaryTrendStats,
  tags: { testid: RUN_ID, scenario: 'room_fill' },
};

const BODY = createBody();

/** Legt den einen Raum an, gibt den Code an die Iterationen weiter. */
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
  // Pro Iteration ein eindeutiger Spieler — `__ITER` läuft 0..19.
  const playerKey = `player_${__ITER}`;
  const payload = submitPayloads[__ITER % submitPayloads.length];

  const r = http.post(
    `${URL}/${data.code}/submit`,
    JSON.stringify({
      player_key: playerKey,
      encrypted_score: payload.encrypted_score,
      encrypted_id: payload.encrypted_id,
    }),
    {
      headers: { 'Content-Type': 'application/json' },
      tags: { endpoint: 'submit', player: playerKey },
      timeout: '60s',
    },
  );
  check(r, { 'submit 200': (x) => x.status === 200 });

  // Dem Hintergrund-Sort kurz Luft geben, bevor der nächste Spieler reinkommt.
  // Sonst stapeln sich Submits auf dem Single-Flight-Sort-Slot und Latenzen
  // werden vom Warten dominiert statt von der reinen Submit-Verarbeitung.
  sleep(SETTLE_SEC);
}
