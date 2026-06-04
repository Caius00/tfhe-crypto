# 🗳️ 03 · Encrypted Voting & Polling

![version](https://img.shields.io/badge/version-0.1.27-blue)

Homomorphes Voting/Polling: der Ersteller kontrolliert die Session, Teilnehmer
müssen freigegeben werden, Stimmen werden serverseitig **ohne Entschlüsselung**
aggregiert. Nur der Ersteller kann die Ergebnisse später lokal entschlüsseln.

## 🌐 Endpoints

| Method | Path | Beschreibung |
|--------|------|--------------|
| `POST` | `/session` | Neue Session anlegen — Body: `{creator_id, server_key, public_key?, questions}`, Response: `{session_id}` |
| `GET`  | `/session/{session_id}` | Fragen + PublicKey holen — Response: `{session_id, questions, public_key}` |
| `POST` | `/join` | Beitrittsanfrage stellen — Body: `{session_id, participant_id, enc_name_chunks?}`, Response: `{status: "pending"}` |
| `GET`  | `/pending/{session_id}/{creator_id}` | Offene Teilnehmer auflisten (nur Ersteller) |
| `POST` | `/approve` | Teilnehmer (de)freigeben — Body: `{session_id, creator_id, participant_id, approved}` |
| `POST` | `/vote` | Verschlüsselte Stimmen abgeben — Body: `{session_id, participant_id, encrypted_votes}` |
| `GET`  | `/status/{session_id}/{participant_id}` | Freigabe-Status abfragen — Response: `approved` \| `pending` \| `not_found` |
| `GET`  | `/results/{session_id}/{creator_id}` | Aggregierte Ergebnisse abrufen (nur Ersteller) — Response: `{encrypted_results, ready}` |
| `POST` | `/finalize/{session_id}/{creator_id}` | Session schließen (nur Ersteller) |
| `GET`  | `/docs` | Swagger UI |
| `GET`  | `/openapi.json` | OpenAPI 3.1 Spec |
| `GET`  | `/version` | Service-Version |
| `GET`  | `/healthz` | Liveness Probe |
| `GET`  | `/readyz` | Readiness Probe |

### Felder

| Feld | Typ | Bedeutung |
|------|-----|-----------|
| `server_key` | Base64(`bincode(CompressedServerKey)`) | erlaubt serverseitige FHE-Aggregation |
| `public_key` | Base64 String | wird Teilnehmern zur Verschlüsselung weitergegeben |
| `questions[i]` | `{id, text, question_type, options?, multiple?}` | `question_type ∈ {bool, single, multiple, numeric}` |
| `encrypted_votes` | `Vec<Vec<String>>` | pro Frage eine Liste von Base64-Ciphertexten |
| `encrypted_results` | `Vec<Vec<String>>` | analog zur Frage-Struktur — pro Frage die aggregierten Ciphertexte |

## ℹ️ Anforderungen

| | |
|--|--|
| 🗄️ Database | — (In-Memory `HashMap`) |
| 📦 Max. Body | 2 GiB (für ServerKey-Upload) |
| 🌐 CORS | `Any` Origin/Method/Header |
