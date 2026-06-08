//! Request- und Response-Schemas plus die Service-weite Fehlerart.
//!
//! Alle verschlüsselten Felder wandern als **Base64-Strings über JSON** —
//! `Vec<String>` für komplette Strings (jedes Element = ein bincode-kodierter
//! `FheUint8`), ein einzelner `String` für skalare Ciphertexts wie `FheBool`.
//! Damit ist die Wire-Form identisch mit dem, was die TFHE-WASM-Bindings im
//! Frontend nativ produzieren — kein Spezial-Format, kein Polyfill.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Antwort von `POST /session` — der Server gibt eine neue Session-ID heraus,
/// unter der der hochgeladene `CompressedServerKey` referenziert werden kann.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateSessionResponse {
    /// Frisch generierte UUID, die das Frontend bei jedem folgenden Request
    /// mitschickt. Lebt nur im Server-Speicher; geht bei Pod-Restart verloren.
    pub session_id: String,
}

/// Body von `POST /session`. Der Server merkt sich den dekomprimierten
/// ServerKey, um homomorphe Operationen (`eq`, `if_then_else`, …) ausführen
/// zu können — er kann damit weder ent- noch verschlüsseln.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateSessionRequest {
    /// Base64-kodierter, bincode-serialisierter `tfhe::CompressedServerKey`.
    pub server_key: String,
}

/// Body von `POST /entry` (Put). TTL ist optional — wenn `None`, gilt der
/// Service-Default aus `TTL_MINUTES`. Die TTL ist absichtlich Klartext-Metadatum:
/// homomorphes Ablaufmanagement gibt es nicht und brauchte Server-Logik, die
/// die TTL kennt.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PutRequest {
    pub session_id: String,
    /// Verschlüsselter Schlüssel — pro Zeichen ein Base64-kodierter
    /// bincode-`FheUint8`. Der Server vergleicht ihn homomorph mit allen
    /// gespeicherten Schlüsseln derselben Session.
    pub key: Vec<String>,
    /// Verschlüsselter Wert — gleiches Format wie `key`.
    pub value: Vec<String>,
    /// Lebensdauer des Eintrags in Sekunden. Wenn nicht gesetzt, nimmt der
    /// Server seinen Default (siehe ENV `TTL_MINUTES`).
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

/// Body von `POST /entry/get`. Liefert den verschlüsselten Wert zurück,
/// dessen verschlüsselter Schlüssel zum übergebenen passt — alles ohne dass
/// der Server jemals erfährt, welcher Eintrag gemeint war.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetRequest {
    pub session_id: String,
    pub key: Vec<String>,
}

/// Antwort von `POST /entry/get`. `value` ist die zeichenweise verschlüsselte
/// Form; das Frontend entschlüsselt sie mit dem ClientKey.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ValueResponse {
    pub value: Vec<String>,
}

/// Body von `POST /entry/exists`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExistsRequest {
    pub session_id: String,
    pub key: Vec<String>,
}

/// Antwort von `POST /entry/exists` — ein einzelner Base64-kodierter
/// bincode-`FheBool`. Das Frontend entschlüsselt zu `true`/`false`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExistsResponse {
    pub exists: String,
}

/// Body von `POST /clear`. Löscht ausschließlich die Einträge dieser Session;
/// der Schlüsselraum anderer Sessions bleibt unberührt.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ClearRequest {
    pub session_id: String,
}

/// Allgemeine Antwort für Endpunkte, die nur eine Status-Nachricht zurückgeben.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MessageResponse {
    pub message: String,
}

/// Service-weite Fehlerart. Wird zentral in `axum`-Responses übersetzt,
/// damit kein Handler manuell `(StatusCode, String)` zusammenbauen muss.
#[derive(Debug)]
pub enum AppError {
    /// Redis-Probleme — Connection, Timeouts, Protocol-Errors.
    Redis(redis::RedisError),
    /// Ungültige Client-Payload (Base64, bincode, fehlende Felder, …).
    BadRequest(String),
    /// Unbekannte/abgelaufene Session-ID.
    Unauthorized,
    /// Schlüssel existiert in dieser Session nicht.
    NotFound,
    /// Alles, was kein Client-Fehler ist und keine Redis-Quelle hat
    /// (z.B. bincode-Serialize-Fehler beim Aufbau der Antwort).
    InternalError(String),
}

impl From<redis::RedisError> for AppError {
    fn from(e: redis::RedisError) -> Self {
        AppError::Redis(e)
    }
}

/// Wandelt einen `AppError` in eine HTTP-Antwort um. Bewusst werden interne
/// Fehlerdetails (Redis-Strings, bincode-Strings) ans Frontend gespiegelt —
/// das ist für einen Demo-Service akzeptabel und beim Debuggen Gold wert.
/// In einer Produkt-Umgebung würde man sensible Felder hier ausblenden.
impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let (status, message) = match self {
            AppError::Redis(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Redis error: {e}"),
            ),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unknown or expired session_id".to_string(),
            ),
            AppError::NotFound => (StatusCode::NOT_FOUND, "key not found".to_string()),
            AppError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        (status, axum::Json(MessageResponse { message })).into_response()
    }
}

/// Damit `aide`/`schemars` Fehlerantworten dokumentieren können.
impl aide::OperationOutput for AppError {
    type Inner = MessageResponse;
}
