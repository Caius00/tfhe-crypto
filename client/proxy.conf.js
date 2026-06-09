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
    execSync(
      `node -e "var n=require('net').createConnection(8080,'localhost');n.on('connect',()=>{n.destroy();process.exit(0)});n.on('error',()=>process.exit(1));setTimeout(()=>process.exit(1),1000)"`,
      { stdio: 'ignore', timeout: 2000 },
    );
    return true;
  } catch {
    return false;
  }
}

const target = isLocalRunning() ? LOCAL : REMOTE;

console.log(
  `\n[Proxy] → ${target === LOCAL ? 'Lokal (localhost:8080)' : 'Remote (159.195.145.100)'}\n`,
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

// HTML-Navigation-Requests (Browser-Refresh, direkte URL-Eingabe) nicht proxyen —
// Angular soll index.html ausliefern und das Routing selbst übernehmen.
// Ausnahme: OpenAPI-Doku-Seiten (/docs) müssen vom Backend kommen, nicht von Angular.
function bypass(req) {
  if (req.url.endsWith('/docs') || req.url.endsWith('/openapi.json')) return null;
  if (req.headers.accept?.includes('text/html')) return '/index.html';
}

module.exports = Object.fromEntries(
  paths.map((path) => [
    path,
    {
      target,
      changeOrigin: true,
      bypass,
      configure: (proxy) => {
        proxy.on('proxyReq', (_, req) => {
          console.log(`[Proxy] ${req.method} ${req.url} → ${target}`);
        });
      },
      ...(target === LOCAL && {
        rewrite: (p) => p.replace(new RegExp(`^${path}`), '') || '/',
      }),
    },
  ]),
);
