mod fhe;
mod statistics;

#[cfg(test)]
mod statistics_tests;

use crate::statistics::DivideByElementCount;
use aide::axum::{routing::post_with, ApiRouter};
use axum::{http::StatusCode, Json, Router};
use schemars::JsonSchema;
use base64::{engine::general_purpose, Engine as _};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::ops::Add;
use tfhe::prelude::{CastInto, FheOrd, IfThenElse};
use tfhe::{CompressedServerKey, FheBool, FheInt16, FheInt32, FheInt64, FheInt8};

/// Anfrage des Clients an den Statistics-Service.
#[derive(Deserialize, Serialize, JsonSchema)]
struct StatisticsRequest {
    /// Jedes Element ist ein Base64-kodiertes, bincode-serialisiertes FHE-Integer.
    /// Der konkrete Typ richtet sich nach `bit_width`.
    encrypted_list: Vec<String>,
    /// Base64-kodierter, bincode-serialisierter CompressedServerKey.
    server_key: String,
    /// Bitbreite der verschlüsselten Eingabewerte: 8, 16 oder 32.
    /// Wird vom Client automatisch anhand des Wertebereichs der Eingabe gewählt.
    bit_width: u8,
}

/// Antwort des Servers an den Client.
/// sum/average haben den nächstbreiteren Typ als die Eingabe (Overflow-Schutz).
/// Alle Felder sind Base64-kodierte, bincode-serialisierte FHE-Ciphertexte.
#[derive(Serialize, Deserialize, JsonSchema)]
struct StatisticsResponse {
    /// FHE-Integer mit doppelter Eingabe-Bitbreite (z.B. Int8-Eingabe → Int16-Summe)
    sum: String,
    /// Klartextzahl — die Listenlänge ist dem Server bereits aus dem Request bekannt
    count: u64,
    /// Gleicher Typ wie die Eingabe
    min: String,
    /// Gleicher Typ wie die Eingabe
    max: String,
    /// FHE-Integer mit doppelter Eingabe-Bitbreite (Overflow-Schutz, siehe sum)
    average: String,
    /// Gleicher Typ wie die Eingabe (Lower Median bei gerader Länge)
    median: String,
    /// Tatsächlich verwendete Bitbreite: 8, 16 oder 32.
    bit_width: u8,
}

/// Deserialisiert eine Liste von Base64-kodierten FHE-Ciphertexten in den konkreten Typ T.
fn deserialize_encrypted_list<T: DeserializeOwned>(
    base64_encoded_list: &[String],
) -> Result<Vec<T>, (StatusCode, String)> {
    base64_encoded_list
        .iter()
        .map(|base64_item| {
            let raw_bytes =
                general_purpose::STANDARD
                    .decode(base64_item)
                    .map_err(|decode_error| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Ungültiger Item-Base64: {}", decode_error),
                        )
                    })?;
            bincode::deserialize(&raw_bytes).map_err(|deserialize_error| {
                (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Fehler beim Deserialisieren des Ciphertexts: {}",
                        deserialize_error
                    ),
                )
            })
        })
        .collect()
}

/// Führt alle homomorphen Berechnungen für eine typisierte verschlüsselte Liste durch
/// und verpackt die Ergebnisse in eine `StatisticsResponse`.
///
/// `InputType` ist der verschlüsselte Eingabetyp (z.B. FheInt16).
/// `WiderOutputType` ist der breitere Typ für Summe und Durchschnitt (z.B. FheInt32),
/// um Overflow bei der Addition vieler Werte zu verhindern.
fn compute_statistics_typed<InputType, WiderOutputType>(
    encrypted_input_list: Vec<InputType>,
    fhe_engine: fhe::FheEngine,
    element_count: u64,
    bit_width: u8,
) -> Result<Json<StatisticsResponse>, (StatusCode, String)>
where
    InputType: Clone + FheOrd + CastInto<WiderOutputType> + Sync + Send + Serialize,
    WiderOutputType:
        Add<WiderOutputType, Output = WiderOutputType> + DivideByElementCount + Send + Serialize,
    FheBool: IfThenElse<InputType>,
{
    let (encrypted_sum, encrypted_min, encrypted_max, encrypted_average, encrypted_median) =
        tokio::task::block_in_place(|| {
            fhe_engine.install(|| {
                let encrypted_sum: WiderOutputType = statistics::sum(&encrypted_input_list);
                let encrypted_min: InputType = statistics::min(&encrypted_input_list);
                let encrypted_max: InputType = statistics::max(&encrypted_input_list);
                let encrypted_average: WiderOutputType = statistics::average(&encrypted_input_list);
                let encrypted_median: InputType = statistics::median(&encrypted_input_list);
                (
                    encrypted_sum,
                    encrypted_min,
                    encrypted_max,
                    encrypted_average,
                    encrypted_median,
                )
            })
        });

    Ok(Json(StatisticsResponse {
        sum: to_base64(&encrypted_sum)?,
        count: element_count,
        min: to_base64(&encrypted_min)?,
        max: to_base64(&encrypted_max)?,
        average: to_base64(&encrypted_average)?,
        median: to_base64(&encrypted_median)?,
        bit_width,
    }))
}

fn to_base64<T: Serialize>(value: &T) -> Result<String, (StatusCode, String)> {
    bincode::serialize(value)
        .map(|serialized_bytes| general_purpose::STANDARD.encode(serialized_bytes))
        .map_err(|serialize_error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Serialisierungsfehler: {}", serialize_error),
            )
        })
}

/// POST /
/// Empfängt eine verschlüsselte Ganzzahlen-Liste und berechnet alle Statistiken homomorph.
/// Die Bitbreite der Eingabe wird im Request-Feld `bit_width` angegeben (8, 16 oder 32).
async fn compute_statistics(
    Json(request): Json<StatisticsRequest>,
) -> Result<Json<StatisticsResponse>, (StatusCode, String)> {
    // 1. Server Key deserialisieren und dekomprimieren
    let server_key_bytes = general_purpose::STANDARD
        .decode(&request.server_key)
        .map_err(|decode_error| {
            (
                StatusCode::BAD_REQUEST,
                format!("Ungültiger ServerKey Base64: {}", decode_error),
            )
        })?;

    let compressed_server_key: CompressedServerKey = bincode::deserialize(&server_key_bytes)
        .map_err(|deserialize_error| {
            (
                StatusCode::BAD_REQUEST,
                format!(
                    "Fehler beim Deserialisieren des ServerKey: {}",
                    deserialize_error
                ),
            )
        })?;

    let fhe_engine = tokio::task::block_in_place(|| {
        fhe::FheEngine::from_server_key(compressed_server_key.decompress())
    })
    .map_err(|engine_error| (StatusCode::BAD_REQUEST, engine_error))?;

    // 2. Leere Liste abfangen (vor der teuren Deserialisierung der Ciphertexte)
    if request.encrypted_list.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Die Liste darf nicht leer sein".to_string(),
        ));
    }

    let element_count = request.encrypted_list.len() as u64;

    // 3. Bitbreite auflösen und an die passende generische Implementierung delegieren.
    //    Die Eingabewerte werden als InputType deserialisiert; Summe und Durchschnitt
    //    laufen intern auf WiderOutputType um Overflow zu verhindern.
    match request.bit_width {
        8 => {
            let encrypted_input_list =
                deserialize_encrypted_list::<FheInt8>(&request.encrypted_list)?;
            compute_statistics_typed::<FheInt8, FheInt16>(
                encrypted_input_list,
                fhe_engine,
                element_count,
                8,
            )
        }
        16 => {
            let encrypted_input_list =
                deserialize_encrypted_list::<FheInt16>(&request.encrypted_list)?;
            compute_statistics_typed::<FheInt16, FheInt32>(
                encrypted_input_list,
                fhe_engine,
                element_count,
                16,
            )
        }
        32 => {
            let encrypted_input_list =
                deserialize_encrypted_list::<FheInt32>(&request.encrypted_list)?;
            compute_statistics_typed::<FheInt32, FheInt64>(
                encrypted_input_list,
                fhe_engine,
                element_count,
                32,
            )
        }
        unsupported_bit_width => Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Ungültige Bitbreite {}: muss 8, 16 oder 32 sein.",
                unsupported_bit_width
            ),
        )),
    }
}

pub(crate) fn create_app() -> Router {
    let (metrics_layer, metrics_router) = metrics_exporter::setup();

    let api_router = ApiRouter::new().api_route(
        "/",
        post_with(compute_statistics, |op| {
            op.description(
                "Compute sum, count, min, max, average and median homomorphically \
                 on an encrypted integer list. Bit width (8/16/32) is chosen by the \
                 client based on the value range.",
            )
            .response::<200, Json<StatisticsResponse>>()
        }),
    );

    openapi_docs::attach(
        api_router,
        "Encrypted Statistics Service",
        "Homomorphic statistics service: computes sum, count, min, max, average and median \
         on an encrypted integer list — the server never sees the values.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .merge(metrics_router)
    // Großes Limit nötig, weil FHE-Ciphertexte sehr groß sind (~1 MB pro Wert)
    .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
    .layer(metrics_layer)
    .layer(observability::http_trace_layer())
}

#[tokio::main]
async fn main() {
    observability::init("encrypted-statistics-service", env!("CARGO_PKG_VERSION"));

    let listening_address = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let tcp_listener = tokio::net::TcpListener::bind(listening_address)
        .await
        .unwrap();
    println!("Statistics Service läuft auf http://{}", listening_address);
    axum::serve(tcp_listener, create_app()).await.unwrap();

    observability::shutdown();
}
