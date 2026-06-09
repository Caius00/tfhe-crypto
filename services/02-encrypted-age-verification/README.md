# 🪪 02 · Encrypted Age Verification

![version](https://img.shields.io/badge/version-0.1.27-blue)

Homomorpher Volljährigkeits-Check: vergleicht ein verschlüsseltes Alter gegen
den festen Schwellwert `18` und gibt einen verschlüsselten Boolean zurück –
der Klartext verlässt den Client nie.

## 🌐 Endpoints

| Method | Path | Beschreibung |
|--------|------|--------------|
| `POST` | `/` | Altersprüfung — Body: `{encrypted_age, server_key}`, Response: `{is_adult}` |
| `GET`  | `/docs` | Swagger UI |
| `GET`  | `/openapi.json` | OpenAPI 3.1 Spec |
| `GET`  | `/version` | Service-Version |
| `GET`  | `/healthz` | Liveness Probe |
| `GET`  | `/readyz` | Readiness Probe |

### Felder

| Feld | Typ | Bedeutung |
|------|-----|-----------|
| `encrypted_age` | Base64(`bincode(FheInt8)`) | mit dem ClientKey verschlüsseltes Alter in Jahren |
| `server_key` | Base64(`bincode(CompressedServerKey)`) | erlaubt FHE-Operationen, kann **nicht** entschlüsseln |
| `is_adult` | Base64(`bincode(FheBool)`) | `true` ⇔ Alter ≥ 18 **und** ≥ 0 |

## ℹ️ Anforderungen

| | |
|--|--|
| 🗄️ Database | — |
| 📦 Max. Body | 2 GiB (für ServerKey-Upload) |
