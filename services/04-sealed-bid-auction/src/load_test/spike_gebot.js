/**
 * ═══════════════════════════════════════════════════════════════════════════════
 * 02_spike_gebot.js – Spike Test (Last-Minute-Bidding Ansturm)
 * ═══════════════════════════════════════════════════════════════════════════════
 */

import http from "k6/http";
import { check, sleep, group } from "k6";
import { Trend, Counter, Rate } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://localhost:8080";
const SERVER_KEY = __ENV.SERVER_KEY || "test_server_key_base64_placeholder";

const spikeLatency = new Trend("spike_latency", true);
const successRate = new Rate("success_rate");
const errorCount = new Counter("errors");

export const options = {
  scenarios: {
    spike: {
      executor: "ramping-vus",
      startVUs: 0,
      stages: [
        { duration: "10s", target: 2 }, // Baseline
        { duration: "2s", target: 100 }, // ← SPIKE: 100 Bieter hämmern SOFORT gleichzeitig rein!
        { duration: "20s", target: 100 }, // Halten
        { duration: "10s", target: 0 }, // Recovery
      ],
      gracefulRampDown: "10s",
      exec: "spikeFlow",
    },
  },
  thresholds: {
    spike_latency: ["p(95)<2000"], // Im ärgsten Spike maximal 2 Sek Latenz erlaubt
    success_rate: ["rate>0.95"], // 95% müssen durchkommen
  },
};

export function spikeFlow() {
  const bidderName = `Spike-Bieter-${Math.random().toString(36).slice(2, 10)}`;

  group("last_minute_bid", () => {
    const payload = JSON.stringify({
      bidder_name: bidderName,
      encrypted_amount: "AgAAAAAAAAD6AwAAAAAAAGVb6bMAAAAAsvAnvS8B...",
      server_key: SERVER_KEY,
    });

    const res = http.post(`${BASE_URL}/auction/gebot`, payload, {
      headers: { "Content-Type": "application/json" },
    });

    spikeLatency.add(res.timings.duration);
    const ok = check(res, { "status 200": (r) => r.status === 200 });

    successRate.add(ok);
    if (!ok) errorCount.add(1);
  });

  sleep(0.5);
}
