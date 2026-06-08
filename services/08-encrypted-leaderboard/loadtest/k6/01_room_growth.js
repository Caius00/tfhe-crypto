/**
 * Room-Growth: jede Minute kommt ein neuer Raum mit einem Spieler dazu,
 * läuft bis der Pod den nächsten Raum nicht mehr annimmt (5xx) oder
 * `abortOnFailure` greift.
 *
 * Pro VU = pro Raum = pro Spieler:
 *   1. Erste Iteration: `POST /create` → eigener Raum-Code
 *   2. Sofort danach: `POST /submit` mit einem festen Player-Key
 *   3. Jede weitere Iteration (alle 9 Minuten): erneut `POST /submit`
 *      ans denselben Raum mit demselben Player-Key.
 *
 * Warum die 9 Minuten? Der Service-Janitor evictet Räume nach 10 Minuten
 * ohne Aktivität (siehe `state.rs:33`, `SESSION_IDLE_TIMEOUT`). Ohne
 * Keepalive würden die ersten Räume immer wieder weggeräumt und der Test
 * würde nie einen echten Wachstums-Limit erreichen. Mit Keepalive bleiben
 * alle erzeugten Räume im RAM, und du siehst im Dashboard linear das Pod-
 * Memory wachsen bis es kippt.
 *
 * Konfiguration (alles per ENV überschreibbar):
 *   MAX_ROOMS=60           — wieviele Räume maximal angelegt werden
 *   STAGGER_SEC=60         — Sekunden zwischen neuen Räumen (1/min Default)
 *   KEEPALIVE_SEC=540      — Sekunden zwischen Re-Submits (= 9 min)
 *   HOLD_DURATION=30m      — wie lange nach Erreichen der MAX_ROOMS gewartet wird
 *
 * Aufruf:
 *   k6 run --out experimental-prometheus-rw 01_room_growth.js \
 *     -e BASE_URL=http://159.195.145.100/leaderboard
 *
 * Im Dashboard beobachten: Pod Memory wächst linear pro Welle (~jede Min ein
 * Sprung). Der Kipp-Punkt ist die Welle bei der das nächste `POST /create`
 * scheitert ODER abortOnFailure die Failure-Rate > 5 % erkennt.
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
const RUN_ID = testId('room-growth');
const MAX_ROOMS = parseInt(__ENV.MAX_ROOMS || '60', 10);
const STAGGER_SEC = parseInt(__ENV.STAGGER_SEC || '60', 10);
const KEEPALIVE_SEC = parseInt(__ENV.KEEPALIVE_SEC || '540', 10);
const HOLD_DURATION = __ENV.HOLD_DURATION || '30m';

export const options = {
  scenarios: {
    growth: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        // Linear hochrampen: 1 neuer VU (= 1 neuer Raum) alle STAGGER_SEC
        { duration: `${MAX_ROOMS * STAGGER_SEC}s`, target: MAX_ROOMS },
        // Halten: VUs schicken weiter Keepalive-Submits, damit nichts evictet wird
        { duration: HOLD_DURATION, target: MAX_ROOMS },
      ],
      gracefulRampDown: '5s',
    },
  },
  thresholds: {
    ...defaultThresholds,
    ...abortOnFailure,
  },
  summaryTrendStats,
  tags: { testid: RUN_ID, scenario: 'room_growth' },
};

const BODY = createBody();

// VU-lokaler State: bleibt über Iterationen desselben VU bestehen, ist aber
// pro VU isoliert. Genau das wollen wir hier — jeder VU besitzt seinen Raum.
let myRoom = null;
let setupFailed = false;

export default function () {
  // Erste Iteration: eigenen Raum anlegen.
  if (myRoom === null && !setupFailed) {
    const r = http.post(`${URL}/create`, BODY, {
      headers: { 'Content-Type': 'application/json' },
      tags: { endpoint: 'create' },
      timeout: '120s',
    });

    if (r.status !== 200) {
      // Konnte keinen Raum mehr anlegen — VU markiert sich als "tot" und
      // idlen einfach bis Test-Ende. Das `create`-Fail zählt in die
      // Failure-Rate und triggert ggf. `abortOnFailure`.
      console.warn(`VU ${__VU} konnte keinen Raum anlegen (status=${r.status}). Idle.`);
      setupFailed = true;
      sleep(KEEPALIVE_SEC);
      return;
    }

    myRoom = r.json('code');
    console.log(`__LEADERBOARD_ROOM_CODE__=${myRoom} (VU ${__VU})`);
  }

  if (setupFailed) {
    sleep(KEEPALIVE_SEC);
    return;
  }

  // Submit als „player_0" — stabil über alle Iterationen, sodass der Raum
  // immer genau einen Spieler hat (und der Service `keep_max` macht statt
  // die Liste mit Karteileichen vollzukleben).
  const payload = submitPayloads[Math.floor(Math.random() * submitPayloads.length)];
  const r = http.post(
    `${URL}/${myRoom}/submit`,
    JSON.stringify({
      player_key: 'player_0',
      encrypted_score: payload.encrypted_score,
      encrypted_id: payload.encrypted_id,
    }),
    {
      headers: { 'Content-Type': 'application/json' },
      tags: { endpoint: 'submit' },
      timeout: '60s',
    },
  );
  check(r, { 'submit 200': (x) => x.status === 200 });

  // Bis zum nächsten Keepalive warten.
  sleep(KEEPALIVE_SEC);
}
