/**
 * URLs für alle Backend-Services.
 *
 * localBase:  Rust-Service läuft lokal (z.B. via `cargo run`)
 * remoteBase: Deployed Service im Kubernetes-Cluster (159.195.145.100)
 * path:       API-Endpunkt-Pfad
 *
 * Der ApiConfigService prüft zur Laufzeit welche Basis erreichbar ist
 * und wählt automatisch die passende URL.
 */
export interface ServiceConfig {
  localBase: string;
  remoteBase: string;
  path: string;
}

const LOCAL_BASE = 'http://localhost:8080';
const REMOTE_BASE = 'http://159.195.145.100';

export const SERVICE_URLS = {
  keyValueStore: {
    localBase: LOCAL_BASE,
    remoteBase: REMOTE_BASE,
    path: '/kv',
  },
  ageVerification: {
    localBase: LOCAL_BASE,
    remoteBase: REMOTE_BASE,
    path: '/age-verification',
  },
  voting: {
    localBase: LOCAL_BASE,
    remoteBase: REMOTE_BASE,
    path: '/voting',
  },
  auction: {
    localBase: LOCAL_BASE,
    remoteBase: REMOTE_BASE,
    path: '/auction',
  },
  statistics: {
    localBase: LOCAL_BASE,
    remoteBase: REMOTE_BASE,
    path: '/statistics',
  },
  genomics: {
    localBase: LOCAL_BASE,
    remoteBase: REMOTE_BASE,
    path: '/genomics',
  },
  imageProcessing: {
    localBase: LOCAL_BASE,
    remoteBase: REMOTE_BASE,
    path: '/image-processing',
  },
  leaderboard: {
    localBase: LOCAL_BASE,
    remoteBase: REMOTE_BASE,
    path: '/leaderboard',
  },
  programExecution: {
    localBase: "ws://localhost:8080",
    remoteBase: "ws://159.195.145.100",
    path: "/execute",
  },
} satisfies Record<string, ServiceConfig>;
