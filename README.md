# tfhe-crypto

> **Jump to:** [🔧 Services](#services) · [📦 Shared](#shared) · [📄 Docs](#docs) · [💬 Commits](#commit-convention)

---

## 🔧 Services

Independent Rust binaries in `services/<name>/`, all exposing health endpoints on port `8080`.

| | Service | Version |
|--|---------|---------|
| [01](services/01-encrypted-key-value-store/README.md) | Encrypted Key-Value Store | ![version](https://img.shields.io/badge/version-0.1.1-blue) |
| [02](services/02-encrypted-age-verification/README.md) | Encrypted Age Verification | ![version](https://img.shields.io/badge/version-0.1.1-blue) |
| [03](services/03-encrypted-voting-polling/README.md) | Encrypted Voting & Polling | ![version](https://img.shields.io/badge/version-0.1.1-blue) |
| [04](services/04-sealed-bid-auction/README.md) | Sealed Bid Auction | ![version](https://img.shields.io/badge/version-0.1.1-blue) |
| [05](services/05-encrypted-statistics-service/README.md) | Encrypted Statistics Service | ![version](https://img.shields.io/badge/version-0.1.1-blue) |
| [06](services/06-encrypted-genomics/README.md) | Encrypted Genomics | ![version](https://img.shields.io/badge/version-0.1.1-blue) |
| [07](services/07-encrypted-image-processing/README.md) | Encrypted Image Processing | ![version](https://img.shields.io/badge/version-0.1.1-blue) |
| [08](services/08-encrypted-leaderboard/README.md) | Encrypted Leaderboard | ![version](https://img.shields.io/badge/version-0.1.1-blue) |
| [09](services/09-encrypted-program-execution/README.md) | Encrypted Program Execution | ![version](https://img.shields.io/badge/version-0.1.1-blue) |

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
```

---

## 💬 Commit Convention

Commits on `main` trigger automatic versioning via [release-plz](https://release-plz.dev). Use the following prefixes:

| Prefix | Example | Version bump |
|--------|---------|--------------|
| `fix:` | `fix: correct encryption output` | Patch `0.1.0 → 0.1.1` |
| `feat:` | `feat: add key rotation` | Minor `0.1.0 → 0.2.0` |
| `feat!:` | `feat!: redesign API` | Major `0.1.0 → 1.0.0` |

After merging to `main`, release-plz opens a Release PR with the updated version. Merging that PR triggers the deploy automatically — no manual version bumping needed.

---

## 📄 Docs

| | |
|--|--|
| [docs/setup/](docs/setup/README.md) | Docker & Compose setup |
| [docs/setup/redis/](docs/setup/redis/README.md) | Redis |
| [docs/setup/postgres/](docs/setup/postgres/README.md) | Postgres |
