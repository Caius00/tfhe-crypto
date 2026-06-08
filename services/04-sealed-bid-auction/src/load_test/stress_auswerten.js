/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * 03_stress_auswerten.js – FHE Stress Test (Maximale CPU-Grenze ermitteln)
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from "k6/http";
import { check, sleep, group } from "k6";
import { Trend, Counter, Rate } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://localhost:8080";

const auswertenLatency = new Trend("auswerten_latency", true);
const successRate = new Rate("success_rate");
const errorCount = new Counter("errors");

export const options = {
  scenarios: {
    stress_fhe: {
      executor: "ramping-vus",
      startVUs: 1,
      stages: [
        { duration: "20s", target: 1 }, // 1 Auktionator wertet aus
        { duration: "30s", target: 3 }, // 3 parallele Berechnungen zeitgleich
        { duration: "30s", target: 6 }, // 6 Berechnungen (CPU gerät unter massiven Druck)
        { duration: "30s", target: 10 }, // Peak: 10 parallele FHE-Schleifen zeitgleich
        { duration: "20s", target: 0 },
      ],
      gracefulRampDown: "20s",
      exec: "fheStressFlow",
    },
  },
  thresholds: {
    auswerten_latency: ["p(95)<60000"], // FHE dauert, maximal 60 Sekunden erlaubt
    success_rate: ["rate>0.95"],
  },
};

export function fheStressFlow() {
  group("fhe_maximum_suche", () => {
    // Hohes HTTP-Timeout gesetzt, da die homomorphe Schleife viel Rechenzeit braucht!
    const res = http.get(`${BASE_URL}/auction/auswerten`, { timeout: "120s" });

    auswertenLatency.add(res.timings.duration);

    const ok = check(res, {
      "status 200": (r) => r.status === 200,
      "has cipher": (r) => {
        try {
          // Prüft, ob der Server wirklich ein verschlüsseltes Ergebnis liefert
          return JSON.parse(r.body).encrypted_result.length > 0;
        } catch (e) {
          return false;
        }
      },
    });

    successRate.add(ok);

    if (!ok) {
      errorCount.add(1);
      console.error(
        `Auswertung blockiert oder fehlgeschlagen! Status: ${res.status}`,
      );
    }
  });

  // Pause zwischen den intensiven Berechnungen der VUs
  sleep(3);
}
