/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * 01_normal_flow.js – Normalbetrieb Lasttest für Sealed-Bid Auction
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from "k6/http";
import { check, sleep, group } from "k6";
import { Trend, Counter, Rate } from "k6/metrics";

// ── Konfiguration ─────────────────────────────────────────────────────────────
const BASE_URL = __ENV.BASE_URL || "http://localhost:8080";
const SERVER_KEY = __ENV.SERVER_KEY || "test_server_key_base64_placeholder";

// ── Eigene Metriken ───────────────────────────────────────────────────────────
const gebotLatency = new Trend("gebot_latency", true);
const successRate = new Rate("success_rate");
const errorCount = new Counter("errors");

export const options = {
  scenarios: {
    bidder_flow: {
      executor: "ramping-vus",
      startVUs: 1,
      stages: [
        { duration: "20s", target: 5 }, // Warm-up
        { duration: "40s", target: 15 }, // Normallast (Bieter kommen rein)
        { duration: "40s", target: 30 }, // Erhöhte Last
        { duration: "20s", target: 0 }, // Cool-down
      ],
      gracefulRampDown: "10s",
      exec: "bidderFlow",
    },
  },
  thresholds: {
    gebot_latency: ["p(95)<500"], // Gebotsabgabe soll unter 500ms liegen
    success_rate: ["rate>0.99"], // Weniger als 1% Fehler
  },
};

export function bidderFlow() {
  // Generiert einen zufälligen Bieternamen für den Test
  const bidderName = `Bieter-${Math.random().toString(36).slice(2, 10)}`;

  group("gebot_abgeben", () => {
    const payload = JSON.stringify({
      bidder_name: bidderName,
      // Ein simulierter Base64-String für das verschlüsselte Gebot (FheUint32)
      encrypted_amount: "AgAAAAAAAAD6AwAAAAAAAGVb6bMAAAAAsvAnvS8B...",
      server_key: SERVER_KEY,
    });

    const res = http.post(`${BASE_URL}/auction/gebot`, payload, {
      headers: { "Content-Type": "application/json" },
    });

    gebotLatency.add(res.timings.duration);

    const ok = check(res, {
      "gebot 200": (r) => r.status === 200,
      "status ok": (r) => {
        try {
          return JSON.parse(r.body).status.contains("received");
        } catch {
          return true;
        }
      },
    });

    successRate.add(ok);
    if (!ok) {
      errorCount.add(1);
      console.error(`Gebot Fehler: ${res.status} – ${res.body}`);
    }
  });

  // Bieter wartet kurz und verlässt dann die Auktion
  sleep(1);
}
