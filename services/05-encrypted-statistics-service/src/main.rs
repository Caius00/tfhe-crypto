mod statistics;

use axum::{routing::post, Json, Router};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use tfhe::prelude::CastInto;
use tfhe::{CompressedServerKey, FheInt64};

/// Anfrage des Clients an den Statistics-Service.
#[derive(Deserialize)]
struct StatisticsRequest {
    /// Jedes Element ist ein Base64-kodiertes, bincode-serialisiertes FheInt64.
    encrypted_list: Vec<String>,
    /// Base64-kodierter, bincode-serialisierter CompressedServerKey.
    server_key: String,
}

/// Antwort des Servers an den Client.
/// Alle verschlüsselten Werte sind FheInt64, damit der Angular-WASM-Client
/// sie mit decryptInt64 entschlüsseln kann. Summe und Durchschnitt werden
/// intern als FheInt128 berechnet (kein Overflow) und dann auf FheInt64 gecastet.
#[derive(Serialize)]
struct StatisticsResponse {
    /// Base64-kodiertes FheInt64 (Summe, intern als Int128 berechnet)
    sum: String,
    /// Klartextzahl – die Listenlänge ist kein Geheimnis
    count: u64,
    /// Base64-kodiertes FheInt64
    min: String,
    /// Base64-kodiertes FheInt64
    max: String,
    /// Base64-kodiertes FheInt64 (Durchschnitt, truncation toward zero)
    average: String,
    /// Base64-kodiertes FheInt64 (Lower Median)
    median: String,
}

/// Hilfsfunktion: serialisiert einen beliebigen Wert zu Base64(bincode).
fn to_base64<T: serde::Serialize>(val: &T) -> Result<String, String> {
    bincode::serialize(val)
        .map(|bytes| general_purpose::STANDARD.encode(bytes))
        .map_err(|e| format!("Serialisierungsfehler: {}", e))
}

/// POST /
/// Empfängt eine verschlüsselte Ganzzahlen-Liste und berechnet alle Statistiken homomorph.
async fn compute_statistics(
    Json(req): Json<StatisticsRequest>,
) -> Result<Json<StatisticsResponse>, String> {
    // 1. Server Key deserialisieren und dekomprimieren
    let sk_bytes = general_purpose::STANDARD
        .decode(&req.server_key)
        .map_err(|e| format!("Ungültiger ServerKey Base64: {}", e))?;

    let compressed: CompressedServerKey = bincode::deserialize(&sk_bytes)
        .map_err(|e| format!("Fehler beim Deserialisieren des ServerKey: {}", e))?;

    let server_key = compressed.decompress();

    // 2. Verschlüsselte Liste deserialisieren
    let enc_list: Vec<FheInt64> = req
        .encrypted_list
        .iter()
        .map(|b64| {
            let bytes = general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| format!("Ungültiger Item-Base64: {}", e))?;
            bincode::deserialize(&bytes)
                .map_err(|e| format!("Fehler beim Deserialisieren von FheInt64: {}", e))
        })
        .collect::<Result<Vec<_>, String>>()?;

    if enc_list.is_empty() {
        return Err("Die Liste darf nicht leer sein".to_string());
    }

    let count = enc_list.len() as u64;

    // 3. Homomorphe Berechnungen – blockierend, da CPU-intensiv.
    //    block_in_place verhindert, dass der Tokio-Threadpool blockiert wird.
    let (enc_sum, enc_min, enc_max, enc_avg, enc_median) =
        tokio::task::block_in_place(|| {
            // Server Key auf dem Hauptthread und auf allen Rayon-Worker-Threads setzen,
            // damit parallele FHE-Operationen in statistics.rs korrekt funktionieren.
            rayon::broadcast(|_| tfhe::set_server_key(server_key.clone()));
            tfhe::set_server_key(server_key);

            // Summe und Durchschnitt intern als Int128 (kein Overflow),
            // dann auf Int64 casten für den WASM-kompatiblen Transport.
            let s:   FheInt64 = statistics::sum(&enc_list).cast_into();
            let mn               = statistics::min(&enc_list);
            let mx               = statistics::max(&enc_list);
            let avg: FheInt64 = statistics::average(&enc_list).cast_into();
            let med              = statistics::median(&enc_list);

            (s, mn, mx, avg, med)
        });

    // 4. Ergebnisse serialisieren und zurücksenden
    Ok(Json(StatisticsResponse {
        sum:     to_base64(&enc_sum)?,
        count,
        min:     to_base64(&enc_min)?,
        max:     to_base64(&enc_max)?,
        average: to_base64(&enc_avg)?,
        median:  to_base64(&enc_median)?,
    }))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", post(compute_statistics))
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        // Großes Limit nötig, weil FHE-Ciphertexte sehr groß sind (~1 MB pro Wert)
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    println!("Statistics Service läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
