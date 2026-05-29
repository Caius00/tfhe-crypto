/**
 * Acceleration-Test: feste Anzahl Spieler, Submit-Intervall wird
 * über die Zeit immer kürzer (von 20 s bis 0.1 s).
 *
 * Drei Setups über ENV-Vars:
 *   ROOMS=1 PLAYERS_PER_ROOM=1   → 1 Spieler — wann ergänzt sich Latenz selbst?
 *   ROOMS=1 PLAYERS_PER_ROOM=20  → voller Raum — wann saturiert ein Raum?  (Default)
 *   ROOMS=5 PLAYERS_PER_ROOM=20  → mehrere Räume — wann kippt es ganz?
 *
 * Stufen (alle gleich lang, je 2 min):
 *   alle 20 s → 10 s → 5 s → 2 s → 1 s → 0.5 s → 0.1 s
 *
 * Beobachte im Dashboard: bei welcher Stufe steigt p95 vom Plateau weg?
 * Das ist die Sättigungsgrenze für genau dieses Setup.
 *
 * Aufruf:
 *   k6 run --out experimental-prometheus-rw 03_acceleration.js \
 *     -e BASE_URL=http://159.195.145.100/leaderboard \
 *     -e ROOMS=1 -e PLAYERS_PER_ROOM=20
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import {
  baseUrl,
  createBody,
  submitPayloads,
  testId,
  summaryTrendStats,
  defaultThresholds,
  abortOnFailure,
} from './_common.js';

const URL = baseUrl();
const ROOMS = parseInt(__ENV.ROOMS || '1', 10);
const PLAYERS_PER_ROOM = parseInt(__ENV.PLAYERS_PER_ROOM || '20', 10);
const TOTAL_VUS = ROOMS * PLAYERS_PER_ROOM;
const RUN_ID = testId(`acceleration-r${ROOMS}-p${PLAYERS_PER_ROOM}`);

// Sleep-Stufen: ab welcher Sekunde des Tests gilt welches Submit-Intervall.
// Jede Stufe 2 min; das gibt im Dashboard klar getrennte Plateaus.
const SLEEP_STAGES = [
  { fromSec: 0, sleep: 20.0 },
  { fromSec: 120, sleep: 10.0 },
  { fromSec: 240, sleep: 5.0 },
  { fromSec: 360, sleep: 2.0 },
  { fromSec: 480, sleep: 1.0 },
  { fromSec: 600, sleep: 0.5 },
  { fromSec: 720, sleep: 0.1 },
];
const TOTAL_DURATION = '15m';

export const options = {
  scenarios: {
    accel: {
      executor: 'constant-vus',
      vus: TOTAL_VUS,
      duration: TOTAL_DURATION,
    },
  },
  thresholds: {
    ...defaultThresholds,
    ...abortOnFailure,
  },
  summaryTrendStats,
  tags: { testid: RUN_ID, scenario: 'acceleration' },
};

const BODY = createBody();

/**
 * Legt ROOMS Räume an, gibt deren Codes + den Start-Zeitstempel an die VUs.
 * Der Start-Zeitstempel ist nötig, damit alle VUs gleich entscheiden können,
 * in welcher Sleep-Stufe wir gerade sind.
 */
export function setup() {
  console.log(`Lege ${ROOMS} Raum/Räume an (~5–10 s pro Raum) …`);
  const codes = [];
  for (let i = 0; i < ROOMS; i++) {
    const r = http.post(`${URL}/create`, BODY, {
      headers: { 'Content-Type': 'application/json' },
      tags: { endpoint: 'create', phase: 'setup' },
      timeout: '120s',
    });
    if (r.status !== 200) {
      throw new Error(`Setup-Raum ${i} fehlgeschlagen: status=${r.status} body=${r.body}`);
    }
    const code = r.json('code');
    codes.push(code);
    console.log(`__LEADERBOARD_ROOM_CODE__=${code} (Raum ${i})`);
  }
  return { codes, startMs: Date.now() };
}

/** Aktuell gültige Sleep-Dauer in Sekunden, basierend auf Test-Laufzeit. */
function currentSleep(startMs) {
  const elapsedSec = (Date.now() - startMs) / 1000;
  let s = SLEEP_STAGES[0].sleep;
  for (const stage of SLEEP_STAGES) {
    if (elapsedSec >= stage.fromSec) s = stage.sleep;
    else break;
  }
  return s;
}

export default function (data) {
  const roomIdx = Math.floor((__VU - 1) / PLAYERS_PER_ROOM);
  const playerIdx = (__VU - 1) % PLAYERS_PER_ROOM;
  const code = data.codes[roomIdx];
  const playerKey = `room_${roomIdx}_player_${playerIdx}`;

  const payload = submitPayloads[Math.floor(Math.random() * submitPayloads.length)];
  const body = JSON.stringify({
    player_key: playerKey,
    encrypted_score: payload.encrypted_score,
    encrypted_id: payload.encrypted_id,
  });

  const r = http.post(`${URL}/${code}/submit`, body, {
    headers: { 'Content-Type': 'application/json' },
    tags: { endpoint: 'submit', room: String(roomIdx) },
    timeout: '60s',
  });
  check(r, { 'submit 200': (x) => x.status === 200 });

  sleep(currentSleep(data.startMs));
}
