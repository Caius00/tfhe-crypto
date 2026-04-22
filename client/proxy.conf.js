/**
 * Proxy-Konfiguration für den Angular Dev-Server (Vite-kompatibel).
 *
 * Prüft synchron beim Start ob localhost:8080 erreichbar ist.
 * Alle API-Pfade werden dann statisch an lokal oder remote weitergeleitet.
 * Der Browser sieht nur localhost:4200 → kein CORS.
 */

const { execSync } = require('child_process');

const LOCAL = 'http://localhost:8080';
const REMOTE = 'http://159.195.145.100';

function isLocalRunning() {
  try {
    execSync('nc -z -w 1 localhost 8080', { stdio: 'ignore', timeout: 2000 });
    return true;
  } catch {
    return false;
  }
}

const target = isLocalRunning() ? LOCAL : REMOTE;

console.log(
  `\n[Proxy] → ${target === LOCAL ? 'Lokal (localhost:8080)' : 'Remote (159.195.145.100)'}\n`
);

const paths = [
  '/kv',
  '/age-verification',
  '/voting',
  '/auction',
  '/statistics',
  '/genomics',
  '/image-processing',
  '/leaderboard',
  '/program-execution',
];

module.exports = Object.fromEntries(
  paths.map((path) => [
    path,
    {
      target,
      changeOrigin: true,
      ...(target === LOCAL && {
        rewrite: (p) => p.replace(new RegExp(`^${path}`), '/'),
      }),
    },
  ])
);
