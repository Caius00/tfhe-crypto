//! HTTP-Handler des Key-Value-Stores.
//!
//! Trennung der Verantwortlichkeiten:
//! - **Decodieren/Encodieren** der Wire-Form (Base64 ↔ bincode-Bytes) passiert
//!   hier am Rand der Funktion, damit `store.rs` reine IO bleibt.
//! - **Homomorphe Operationen** (FHE-Eq, FHE-`if_then_else`, …) sind teuer
//!   und CPU-bound, daher in `tokio::task::block_in_place` gekapselt, damit
//!   sie den async-Executor nicht blockieren.
//! - **Fehlerpfade** geben durchgängig `AppError` zurück; `unwrap` ist tabu.

use crate::codec::{b64_decode_chunks, b64_decode_single, b64_encode_single};
use crate::custom_fhe_ascii_string::{CompressedCustomFheAsciiString, CustomFheAsciiString};
use crate::models::{
    AppError, ClearRequest, CreateSessionRequest, CreateSessionResponse, ExistsRequest,
    ExistsResponse, GetRequest, MessageResponse, PutRequest, ValueResponse,
};
use crate::store::SharedState;
use axum::extract::State;
use axum::Json;
use std::ops::BitOr;
use tfhe::prelude::{FheEq, FheTrivialEncrypt, IfThenElse};
use tfhe::{CompressedServerKey, FheBool};

/// `POST /session` — Client lädt seinen `CompressedServerKey` hoch, Server
/// hängt ihn unter einer neuen UUID in die in-memory Session-Map. Antwort
/// enthält die Session-ID, die bei jedem folgenden Request mitgesendet wird.
pub async fn create_session(
    State(state): State<SharedState>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, AppError> {
    let key_bytes = b64_decode_single(&body.server_key)?;

    // Dekomprimieren ist CPU-intensiv (Hunderte ms bis Sekunden), gehört auf
    // einen Blocking-Thread, damit der async-Executor nicht steht.
    let server_key = tokio::task::block_in_place(|| -> Result<_, AppError> {
        let compressed: CompressedServerKey = bincode::deserialize(&key_bytes)
            .map_err(|e| AppError::BadRequest(format!("invalid CompressedServerKey: {e}")))?;
        Ok(compressed.decompress())
    })?;

    // Original-Bytes wandern nach Redis (überleben Pod-Restart), der dekomprimierte
    // Key in den In-Memory-Hot-Cache.
    let session_id = state.register_session(key_bytes, server_key).await?;
    tracing::info!(%session_id, "new session created");

    Ok(Json(CreateSessionResponse { session_id }))
}

/// `POST /entry` — verschlüsselten Eintrag mit (Klartext-)TTL ablegen.
/// Wir prüfen nur, dass die Session existiert (Server-Key vorhanden), und
/// reichen die Chunks ohne homomorphe Operation an Redis weiter.
pub async fn put_entry(
    State(state): State<SharedState>,
    Json(body): Json<PutRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    // Reine Existenz-Prüfung der Session — wir brauchen den Key hier nicht
    // zum Rechnen, aber wir wollen Requests mit unbekannter Session sauber
    // mit 401 ablehnen.
    let _ = state.fetch_server_key(&body.session_id).await?;

    let key_chunks = b64_decode_chunks(&body.key)?;
    let value_chunks = b64_decode_chunks(&body.value)?;
    let ttl_sec = body.ttl_seconds.unwrap_or(state.default_ttl_sec);

    state
        .put_entry(&body.session_id, key_chunks, value_chunks, ttl_sec)
        .await?;

    tracing::debug!(
        session_id = %body.session_id,
        ttl_sec,
        "entry stored"
    );

    Ok(Json(MessageResponse {
        message: "stored".to_string(),
    }))
}

/// `POST /entry/get` — gibt den Wert zurück, dessen Schlüssel zum übergebenen
/// passt. Die Match-Auswahl passiert vollständig homomorph: der Server lernt
/// weder, welcher Eintrag gemeint war, noch ob er existiert.
pub async fn get_entry(
    State(state): State<SharedState>,
    Json(body): Json<GetRequest>,
) -> Result<Json<ValueResponse>, AppError> {
    let server_key = state.fetch_server_key(&body.session_id).await?;
    let needle_chunks = b64_decode_chunks(&body.key)?;
    let entries = state.load_session_entries(&body.session_id).await?;

    if entries.is_empty() {
        return Err(AppError::NotFound);
    }

    // Heavy-Compute: ServerKey aktivieren, dann pro Eintrag homomorph
    // vergleichen und konditional in den laufenden Wert mischen.
    let compressed_value = tokio::task::block_in_place(move || -> Result<Vec<Vec<u8>>, AppError> {
        tfhe::set_server_key(server_key);

        let needle = CompressedCustomFheAsciiString::from_chunks(needle_chunks).decompress()?;

        let mut acc_match: Option<FheBool> = None;
        let mut acc_value: Option<CustomFheAsciiString> = None;

        for entry in entries {
            let stored_key =
                CompressedCustomFheAsciiString::from_chunks(entry.key_chunks).decompress()?;
            let stored_value =
                CompressedCustomFheAsciiString::from_chunks(entry.value_chunks).decompress()?;

            let match_this = stored_key.eq(needle.clone());

            match (&acc_match, &acc_value) {
                (None, _) => {
                    acc_match = Some(match_this);
                    acc_value = Some(stored_value);
                }
                (Some(prev_match), Some(prev_value)) => {
                    // Wenn die laufende Akkumulator-Wert-Länge nicht zum
                    // aktuellen Eintrag passt, können wir homomorph nicht
                    // mischen — das wäre eine semantisch ungeklärte Situation
                    // bei heterogenen Wertlängen pro Session. Spec dokumentiert
                    // diese Limitation; wir lehnen den Request sauber ab.
                    if prev_value.chars.len() != stored_value.chars.len() {
                        return Err(AppError::BadRequest(
                            "stored values have mismatching lengths — \
                             use uniform value lengths within a session"
                                .into(),
                        ));
                    }

                    let new_match = prev_match.clone().bitor(match_this.clone());
                    let new_value = match_this.if_then_else(&stored_value, prev_value);
                    acc_match = Some(new_match);
                    acc_value = Some(new_value);
                }
                _ => unreachable!("acc_match and acc_value are always set together"),
            }
        }

        // Der Server kann den Match-Status nicht prüfen (er ist verschlüsselt) —
        // er gibt einfach den Akkumulator zurück. Falls der Schlüssel nicht
        // existiert, ist `acc_value` Müll, aber der Client erkennt das beim
        // Entschlüsseln (z.B. via vorgelagertem `/entry/exists`).
        let value = acc_value.expect("entries non-empty, acc_value must be set");
        Ok(value.compress()?.chunks)
    })?;

    Ok(Json(ValueResponse {
        value: compressed_value
            .iter()
            .map(|c| b64_encode_single(c))
            .collect(),
    }))
}

/// `POST /entry/exists` — verschlüsseltes Bool zurück: `true` (verschlüsselt)
/// genau dann, wenn mindestens ein gespeicherter Schlüssel matched.
pub async fn entry_exists(
    State(state): State<SharedState>,
    Json(body): Json<ExistsRequest>,
) -> Result<Json<ExistsResponse>, AppError> {
    let server_key = state.fetch_server_key(&body.session_id).await?;
    let needle_chunks = b64_decode_chunks(&body.key)?;
    let entries = state.load_session_entries(&body.session_id).await?;

    let exists_bytes = tokio::task::block_in_place(move || -> Result<Vec<u8>, AppError> {
        tfhe::set_server_key(server_key);
        let needle = CompressedCustomFheAsciiString::from_chunks(needle_chunks).decompress()?;

        // Triviales `false` als Startwert — wird durch jeden Match per OR auf
        // homomorphes `true` gehoben. Bei null Einträgen bleibt das Ergebnis
        // entsprechend `false`.
        let mut result = FheBool::encrypt_trivial(false);
        for entry in entries {
            let stored_key =
                CompressedCustomFheAsciiString::from_chunks(entry.key_chunks).decompress()?;
            result = result.bitor(stored_key.eq(needle.clone()));
        }

        bincode::serialize(&result)
            .map_err(|e| AppError::InternalError(format!("bincode FheBool: {e}")))
    })?;

    Ok(Json(ExistsResponse {
        exists: b64_encode_single(&exists_bytes),
    }))
}

/// `POST /clear` — löscht alle Einträge dieser Session aus Redis. Der ServerKey
/// bleibt registriert; der Client kann weitere Puts machen, ohne `/session`
/// erneut aufzurufen.
pub async fn clear_entries(
    State(state): State<SharedState>,
    Json(body): Json<ClearRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    // Auch hier explizit prüfen — keine Drittpartei darf andere Sessions
    // räumen, selbst wenn sie die ID erraten würde, weil Sessions ohne
    // hinterlegten ServerKey nicht aktiv sind.
    let _ = state.fetch_server_key(&body.session_id).await?;
    let deleted = state.clear_session(&body.session_id).await?;
    tracing::info!(session_id = %body.session_id, deleted, "session cleared");
    Ok(Json(MessageResponse {
        message: format!("cleared {deleted} entries"),
    }))
}

