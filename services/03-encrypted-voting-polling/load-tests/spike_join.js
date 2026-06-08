/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * spike_join.js – Spike-Test für Join- und Polling-Verhalten
 * ═══════════════════════════════════════════════════════════════════════════════
 *
 * Was wird getestet?
 *   Dieses Szenario simuliert einen plötzlichen Anstieg der Teilnehmeraktivität
 *   innerhalb einer laufenden Session. Nach einer kurzen Baseline-Phase steigt
 *   die Anzahl der virtuellen Nutzer (VUs) innerhalb von zwei Sekunden von
 *   5 auf 500 an.
 *
 *   Jede VU führt genau eine Join-Anfrage aus und geht anschließend in ein
 *   kontinuierliches Polling des eigenen Teilnehmerstatus über. Dadurch wird
 *   sowohl die Aufnahme neuer Teilnehmer als auch die Stabilität des Systems
 *   unter anschließender Dauerlast untersucht.
 *
 *   Ziel des Tests ist es, die Leistungsfähigkeit der unverschlüsselten
 *   Standard-Endpunkte unter hoher Parallelität zu bewerten und zu beobachten,
 *   ob steigende Last zu erhöhten Antwortzeiten oder funktionalen Fehlern führt.
 *
 * Endpunkte:
 *   - POST /join
 *       Jeder Teilnehmer tritt genau einmal der Session bei.
 *
 *   - GET /status
 *       Nach dem Join fragt jeder Teilnehmer seinen Status in einem festen
 *       Polling-Intervall von 200 ms ab.
 *
 * Lastprofil:
 *   - 10 s:  5 VUs (Baseline)
 *   -  2 s:  Anstieg von 5 auf 500 VUs (Spike)
 *   - 30 s:  500 VUs (Dauerlast)
 *   - 10 s:  Rückgang von 500 auf 5 VUs (Recovery)
 *   - 20 s:  5 VUs (Nachlauf)
 *
 * Voraussetzungen:
 *   1. Das Backend läuft.
 *   2. Eine offene (nicht finalisierte) Session existiert.
 *   3. Die session_id ist bekannt.
 *
 * Ausführen (lokal):
 *   k6 run --env BASE_URL=http://localhost:8080 \
 *           --env SESSION_ID=<uuid> \
 *           --out json=results/spike_join.json \
 *           services/03-encrypted-voting-polling/load-tests/spike_join.js
 *
 * Ausführen (Remote):
 * k6 run --env BASE_URL=http://159.195.145.100/voting --env SESSION_ID=81aa96de-3b05-42e9-bab3-527a61239774 --out json=results/spike_join.json services/03-encrypted-voting-polling/load-tests/spike_join.js
 *
 * * Mess-Setup:
 *  *   - Tool:      k6
 *  *   - TFHE:      ConfigBuilder::default()
 *  *   - Datum:     <vor dem Test eintragen>
 *  *   - Server:    <lokal / Netcup>
 *  *   - CPU/RAM:   <Serverspezifikationen eintragen>
 *  *
 *  * Erwartetes Verhalten:
 *  *   - Die Join- und Status-Endpunkte bleiben auch unter hoher Last stabil.
 *  *   - Die Antwortzeiten steigen während der Spike- und Dauerlastphase nur
 *  *     moderat an.
 *  *   - Nach Reduktion der Last normalisieren sich die Antwortzeiten wieder.
 *  *   - Es treten keine oder nur sehr wenige fehlgeschlagene Requests auf.
 *  *
 *  * Hinweis:
 *  *   Da jede VU nach dem Join in eine Endlosschleife zum Status-Polling
 *  *   übergeht, werden Iterationen nicht regulär abgeschlossen. Die von k6
 *  *   ausgegebene Warnung über unterbrochene Iterationen ist daher erwartetes
 *  *   Verhalten und kein Hinweis auf einen Fehler im Test.
 *  * ═══════════════════════════════════════════════════════════════════════════════
 *  */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Trend, Counter, Rate } from 'k6/metrics';

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL   = __ENV.BASE_URL   || 'http://159.195.145.100/voting';
const SESSION_ID = __ENV.SESSION_ID || '';

if (!SESSION_ID) {
    throw new Error('SESSION_ID fehlt! Bitte --env SESSION_ID=<uuid> angeben.');
}

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const joinLatency   = new Trend('join_latency',   true);
const statusLatency = new Trend('status_latency', true);
const errorCount    = new Counter('errors');
const successRate   = new Rate('success_rate');

// ── Lastkurve: Spike ──────────────────────────────────────────────────────────
// ... (Deine Imports, Konfigurationen und Metriken bleiben exakt gleich)

export const options = {
    scenarios: {
        spike: {
            executor: 'ramping-vus',
            startVUs: 0,
            stages: [
                { duration: '10s', target: 5   },  // Baseline
                { duration: '2s',  target: 500 },  // ← Spike: 100 VUs kommen angerannt
                { duration: '30s', target: 500 },  // Halten: Hier wollen wir sehen, wie sie pollen!
                { duration: '10s', target: 5   },  // Recovery
                { duration: '20s', target: 5   },
            ],
            gracefulRampDown: '10s',
            exec: 'spikeFlow',
        },
    },
    thresholds: {
        'join_latency':   ['p(95)<2000'],
        'status_latency': ['p(95)<500'],
        'success_rate':   ['rate>0.95'],
    },
};

export function spikeFlow() {
    const participantId = `p-spike-${Math.random().toString(36).slice(2, 10)}`;

    // 1. Der Join passiert exakt EINMAL pro VU, wenn sie instanziiert wird
    group('join_spike', () => {
        const res = http.post(
            `${BASE_URL}/join`,
            JSON.stringify({
                session_id: SESSION_ID,
                participant_id: participantId,
                enc_name_chunks: null,
            }),
            { headers: { 'Content-Type': 'application/json' } }
        );

        joinLatency.add(res.timings.duration);
        const ok = check(res, {
            'join 200': r => r.status === 200,
            'join pending': r => {
                try { return JSON.parse(r.body).status === 'pending'; } catch { return false; }
            },
        });
        successRate.add(ok);
        if (!ok) {
            errorCount.add(1);
            console.error(`Join Fehler: ${res.status}`);
        }
    });

    // Kurz warten nach dem Join
    sleep(0.5);

    // 2. Unendliche Polling-Schleife:
    // Sobald die VU beigetreten ist, verlässt sie diese Funktion NICHT mehr,
    // sondern pollt alle 2 Sekunden den Status, bis k6 die VU terminiert.
    while (true) {
        group('status_spike', () => {
            const res = http.get(`${BASE_URL}/status/${SESSION_ID}/${participantId}`);

            statusLatency.add(res.timings.duration);
            const ok = check(res, { 'status 200': r => r.status === 200 });
            successRate.add(ok);
            if (!ok) {
                errorCount.add(1);
                // Wirft die Exception und bricht die aktuelle Iteration der VU sofort ab
                throw new Error(`Status-Polling fehlgeschlagen! HTTP-Code: ${res.status} für Participant: ${participantId}`);
            }

        });

        // Simuliert das echte Polling-Intervall deines Clients (z.B. 2 Sekunden)
        sleep(0.2);
    }
}