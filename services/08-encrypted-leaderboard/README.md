# 🏆 08 · Encrypted Leaderboard

![version](https://img.shields.io/badge/version-0.1.21-blue)

Rangliste mit verschlüsselten Scores: der Server speichert, sortiert und durchsucht
Einträge ohne sie jemals lesen zu können — nur der Raum-Ersteller kann entschlüsseln.

## 💡 Idee in 30 Sekunden

1. **E (Ersteller)** generiert lokal ein FHE-Schlüsselpaar (`ClientKey` + `ServerKey`)
   und einen `PublicKey`. Der `ClientKey` verlässt das Gerät niemals.
2. E sendet `ServerKey` + `PublicKey` an den Service und bekommt einen 6-stelligen
   Raum-Code zurück.
3. **Spieler** holen sich den `PublicKey` über den Code und schicken ihre
   verschlüsselten Scores + IDs. Sie können die Rangliste selbst nicht lesen.
4. Der Server hält die Einträge **immer verschlüsselt**, sortiert sie via
   homomorphem Sortier-Netzwerk und kann auf Anfrage den Rang einer Kennung
   berechnen — alles ohne Klartext zu kennen.
5. Nur **E** kann mit seinem `ClientKey` die Rangliste oder Rang-Antworten
   entschlüsseln.

## 🌐 Endpoints

| Method | Path | Beschreibung |
|--------|------|--------------|
| `POST` | `/create` | Raum anlegen — Body: `{server_key, public_key}` (beide Base64), Response: `{code}` |
| `GET`  | `/{code}/public-key` | PublicKey des Raums — Response: `{public_key}` (Base64) |
| `POST` | `/{code}/submit` | Score abgeben — Body: `{player_key, encrypted_score, encrypted_id}` |
| `GET`  | `/{code}/entries` | Aktuelle (sortierte) Liste — Response: `{entries: [{encrypted_score, encrypted_id}]}` |
| `POST` | `/{code}/rank` | Rang einer Kennung abfragen — Body: `{encrypted_id}`, Response: `{matches: [b64,…]}` |
| `GET`  | `/docs` | Swagger UI |
| `GET`  | `/openapi.json` | OpenAPI 3.1 Spec |
| `GET`  | `/version` | Service-Version |
| `GET`  | `/healthz` | Liveness Probe |
| `GET`  | `/readyz` | Readiness Probe |

### Felder

| Feld | Typ | Bedeutung |
|------|-----|-----------|
| `server_key` | Base64(`bincode(CompressedServerKey)`) | erlaubt FHE-Operationen, kann **nicht** entschlüsseln |
| `public_key` | Base64(`bincode(TfheCompactPublicKey)`) | wird Spielern weitergegeben |
| `player_key` | String | **Klartext** — nur zur Server-Side-Deduplizierung pro Spieler |
| `encrypted_score` | Base64(`bincode(FheUint16)`) | 0–65 535 |
| `encrypted_id` | Base64(`bincode(FheUint8)`) | 0–255 |
| `matches[i]` | Base64(`bincode(FheBool)`) | `true` ⇔ Eintrag an Rang i+1 hat die abgefragte Kennung |

## ⚙️ Wie es intern funktioniert

| Schritt | Was passiert |
|---------|--------------|
| **Create** | `CompressedServerKey` wird einmal dekomprimiert (teuer) und in die `FheEngine` der Session gelegt. Eigener rayon-Pool, der den `ServerKey` per `start_handler` auf jedem Worker-Thread setzt → Kein Decompress mehr pro Request. |
| **Submit (neu)** | Eintrag wird direkt aufgenommen — keine FHE-Op nötig. |
| **Submit (existiert)** | FHE-Op `keep_max(alt, neu)` → der höhere Score bleibt. Dauert ~3–5 s auf Standard-Hardware. |
| **Sort** | Hintergrund-Task, **Single-Flight**: läuft maximal eine Sortierung gleichzeitig pro Raum, bündelt mehrere Submits zu „aktueller Pass + ein Nachzieh-Pass". Algorithmus: Batcher's Odd-Even Mergesort mit parallelen Layers über den FHE-Pool. |
| **Entries** | Liefert die zuletzt fertig sortierte Sicht; falls noch nicht vorhanden, die Insertion-Order (damit der erste Spieler ohne Sort-Wartezeit auftaucht). |
| **Rank** | Pro Position der sortierten Liste: ein FHE-Vergleich `id == target` → verschlüsselter Boolean. E entschlüsselt clientseitig, jede `true`-Position ist ein Rang. Funktioniert auch bei Mehrfach-Treffern. |

## 🧪 Tests ausführen

Empfohlen über nextest — die Workspace-`.config/nextest.toml` setzt diesen
Service auf serielle Ausführung (FHE-ServerKeys sind groß, parallel = RAM-Druck):

```sh
cargo nextest run --release -p encrypted-leaderboard
```

Mit Coverage:

```sh
cargo llvm-cov nextest --release -p encrypted-leaderboard --summary-only
```

Ohne nextest geht es auch, dann muss der Thread-Limit explizit dazu:

```sh
cargo test --release -p encrypted-leaderboard -- --test-threads=1
```

> Das einmalige Generieren der TFHE-Keys dauert ~30–90 s.
> Im Debug-Modus sind FHE-Operationen ca. 10× langsamer als in Release.

## ℹ️ Anforderungen

| | |
|--|--|
| 🗄️ Database | — |
| ⏱️ Submit-Latenz | ~3–5 s bei Re-Submit (FHE max), ~1 ms bei neuem Spieler |
| ⏱️ Sort-Latenz | ~70 s bei 20 Einträgen (Hintergrund) |
| 👥 Max. Spieler | 20 pro Raum |
| 📦 Max. Body | 2 GiB (für ServerKey-Upload) |
