/**
 * Room-Fill (Acceleration mit wachsender Spielerzahl): 1 Raum, in dem
 * die Spielerzahl pro Runde um eins steigt — und innerhalb jeder Runde
 * das Submit-Tempo stufenweise von „alle 10 s" auf „jede Sekunde" anzieht.
 *
 * Ablauf:
 *   Runde 1 (Aktivität, 10 min):
 *     - 1 Spieler
 *     - Minute 0–1: alle 10 s ein Submit
 *     - Minute 1–2: alle  9 s
 *     - Minute 2–3: alle  8 s
 *     - …
 *     - Minute 9–10: jede Sekunde
 *   Pause 2 min (kein Submit — Trennzeichen für's Dashboard)
 *   Runde 2:
 *     - 2 Spieler, beide laufen die gleichen 10 Stufen parallel
 *   Pause 2 min
 *   Runde 3 … bis Runde 20 (alle 20 Spieler aktiv)
 *
 * Pro Runde: 10 × 1 min Aktivität + 2 min Pause = 12 min.
 * Gesamt: 20 × 12 min = 4 Stunden bei Defaults.
 *
 * Test endet automatisch bei `abortOnFailure` (>5 % Fehler) oder wenn alle
 * 20 Runden durch sind. Da `MAX_ENTRIES = 20` (siehe `state.rs:25`), kann
 * die letzte Runde den Raum exakt füllen.
 *
 * Konfiguration über ENV (kürzer für Smoke-Test):
 *   MAX_PLAYERS=20          — Endpunkt der Wachstumskurve
 *   STAGE_DURATION_SEC=60   — Dauer jeder Tempo-Stufe (10 Stufen pro Runde)
 *   PAUSE_DURATION_SEC=120  — Pause zwischen Runden (Trennzeichen)
 *
 * Beispiel-Smoke-Test (~12 min statt 4 h):
 *   k6 run … 02_room_fill.js -e MAX_PLAYERS=3 -e STAGE_DURATION_SEC=20 -e PAUSE_DURATION_SEC=30
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
  abortOnFailure,
} from './_common.js';

const URL = baseUrl();
const RUN_ID = testId('room-fill');
const MAX_PLAYERS = parseInt(__ENV.MAX_PLAYERS || '20', 10);
const STAGE_DURATION_SEC = parseInt(__ENV.STAGE_DURATION_SEC || '60', 10);
const PAUSE_DURATION_SEC = parseInt(__ENV.PAUSE_DURATION_SEC || '120', 10);

// Pro Stufe die Sekunden-Pause zwischen zwei Submits desselben Spielers.
const SLEEP_STAGES_SEC = [10, 9, 8, 7, 6, 5, 4, 3, 2, 1];

const ROUND_ACTIVITY_SEC = STAGE_DURATION_SEC * SLEEP_STAGES_SEC.length;
const ROUND_TOTAL_SEC = ROUND_ACTIVITY_SEC + PAUSE_DURATION_SEC;

// Ramping-Stages bauen: am Anfang jeder Runde wird ein neuer VU dazugenommen.
// Die kurze 5-s-Ramp gibt k6 Zeit, den VU zu starten, ohne dass die ersten
// Submits außer Takt mit den Stufen geraten.
const stages = [];
for (let round = 1; round <= MAX_PLAYERS; round++) {
  stages.push({ duration: '5s', target: round });
  stages.push({ duration: `${ROUND_TOTAL_SEC - 5}s`, target: round });
}

export const options = {
  scenarios: {
    fill: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages,
      gracefulRampDown: '5s',
    },
  },
  thresholds: {
    ...defaultThresholds,
    ...abortOnFailure,
  },
  summaryTrendStats,
  tags: { testid: RUN_ID, scenario: 'room_fill' },
};

const BODY = createBody();

/** Legt den einen Raum an, gibt den Code + Start-Zeitstempel an die VUs. */
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
  return { code, startMs: Date.now() };
}

/**
 * Zerlegt den seit Test-Start vergangenen Zeitraum in (Runde, Phase, Sleep).
 * - `inActivity` true: gerade läuft die Submit-Phase
 * - `sleepSec`: passendes Sleep-Intervall (Stage-Wert) bzw. Rest-Pause
 */
function currentPhase(elapsedSec) {
  const elapsedInRound = elapsedSec % ROUND_TOTAL_SEC;

  if (elapsedInRound >= ROUND_ACTIVITY_SEC) {
    // Pause-Phase: bis zum Start der nächsten Aktivitäts-Phase schlafen.
    return {
      inActivity: false,
      sleepSec: ROUND_TOTAL_SEC - elapsedInRound + 0.1,
    };
  }

  const stageIdx = Math.min(
    Math.floor(elapsedInRound / STAGE_DURATION_SEC),
    SLEEP_STAGES_SEC.length - 1,
  );
  return {
    inActivity: true,
    sleepSec: SLEEP_STAGES_SEC[stageIdx],
  };
}

export default function (data) {
  const elapsedSec = (Date.now() - data.startMs) / 1000;
  const phase = currentPhase(elapsedSec);

  // Pause: einfach durchschlafen ohne Submit.
  if (!phase.inActivity) {
    sleep(phase.sleepSec);
    return;
  }

  // VU N stellt Spieler N-1 dar — stabil über alle Iterationen.
  const playerKey = `player_${__VU - 1}`;
  const payload = submitPayloads[Math.floor(Math.random() * submitPayloads.length)];

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

  sleep(phase.sleepSec);
}
