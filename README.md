# tfhe-crypto

> **Jump to:** [🔧 Services](#services) · [📦 Shared](#shared) · [📄 Docs](#docs) · [💬 Commits](#commit-convention)

---

## 🔧 Services

Independent Rust binaries in `services/<name>/`, all exposing health endpoints on port `8080`.

| | Service | Version | API |
|--|---------|---------|-----|
| [01](services/01-encrypted-key-value-store/README.md) | Encrypted Key-Value Store | ![version](https://img.shields.io/badge/version-0.1.20-blue) | `http://159.195.145.100/kv` |
| [02](services/02-encrypted-age-verification/README.md) | Encrypted Age Verification | ![version](https://img.shields.io/badge/version-0.1.25-blue) | `http://159.195.145.100/age-verification` |
| [03](services/03-encrypted-voting-polling/README.md) | Encrypted Voting & Polling | ![version](https://img.shields.io/badge/version-0.1.33-blue) | `http://159.195.145.100/voting` |
| [04](services/04-sealed-bid-auction/README.md) | Sealed Bid Auction | ![version](https://img.shields.io/badge/version-0.1.24-blue) | `http://159.195.145.100/auction` |
| [05](services/05-encrypted-statistics-service/README.md) | Encrypted Statistics Service | ![version](https://img.shields.io/badge/version-0.1.26-blue) | `http://159.195.145.100/statistics` |
| [06](services/06-encrypted-genomics/README.md) | Encrypted Genomics | ![version](https://img.shields.io/badge/version-0.1.25-blue) | `http://159.195.145.100/genomics` |
| [07](services/07-encrypted-image-processing/README.md) | Encrypted Image Processing | ![version](https://img.shields.io/badge/version-0.1.24-blue) | `http://159.195.145.100/image-processing` |
| [08](services/08-encrypted-leaderboard/README.md) | Encrypted Leaderboard | ![version](https://img.shields.io/badge/version-0.1.27-blue) | `http://159.195.145.100/leaderboard` |
| [09](services/09-encrypted-program-execution/README.md) | Encrypted Program Execution | ![version](https://img.shields.io/badge/version-0.1.23-blue) | `http://159.195.145.100/program-execution` |

---

## 🚀 Infrastructure

| | Link |
|--|------|
| ArgoCD | [http://159.195.145.100/argocd](http://159.195.145.100/argocd) |
| Grafana Dashboards | [http://159.195.145.100/grafana/dashboards](http://159.195.145.100/grafana/dashboards) |
| Traefik Dashboard | port-forward only (see below) |

**Traefik Dashboard:**
```bash
kubectl port-forward -n traefik pod/$(kubectl get pod -n traefik -o name | head -1 | cut -d/ -f2) 9000:9000
# → http://localhost:9000/dashboard/
```

---

## 🖥️ Client

[`client/`](client/README.md) — Client-side code for interacting with the services.

---

## 📦 Shared

Code used across **all** services lives here — service-specific logic stays in the service itself.

### [`shared/health`](shared/health/)

```
GET /healthz   liveness probe
GET /readyz    readiness probe
GET /version   current service version
```

---

## 💬 Commit Convention

| Prefix | Example | When to use |
|--------|---------|-------------|
| `fix:` | `fix: correct encryption output` | Bug fix |
| `feat:` | `feat: add key rotation` | New feature |
| `chore:` | `chore: update dependencies` | Maintenance, no functional change |

Merging to `main` automatically builds and deploys all changed services. The patch version is bumped automatically (`0.1.0 → 0.1.1`). For a minor or major bump, update the version in `Cargo.toml` manually before merging.

---

## 📄 Docs

| | |
|--|--|
| [docs/setup/](docs/setup/README.md) | Docker & Compose setup |
| [docs/setup/redis/](docs/setup/redis/README.md) | Redis |
| [docs/setup/postgres/](docs/setup/postgres/README.md) | Postgres |
