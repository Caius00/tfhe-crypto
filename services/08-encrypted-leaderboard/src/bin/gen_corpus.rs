//! Corpus-Generator für Loadtests des Leaderboard-Services.
//!
//! Erzeugt einmalig:
//! - `server_key.b64`        — komprimierter ServerKey (base64), wie der Service ihn erwartet
//! - `public_key.b64`        — Platzhalter; der Service speichert ihn nur, nutzt ihn nicht selbst
//! - `client_key.bin`        — ClientKey für spätere lokale Entschlüsselung
//! - `create_body.json`      — fertiger Request-Body für `POST /create`
//! - `submit_payloads.ndjson`— eine Zeile pro `{player_key, encrypted_score, encrypted_id}`
//! - `meta.json`             — TFHE-Parameter, Zähler, Datum, Git-SHA (Reproduzierbarkeit)
//!
//! TFHE-Keygen + 2N Encrypts brauchen ~1–2 Minuten. Ausgabe geht nach `--out`.
//!
//! Aufruf:
//!   cargo run --release -p encrypted-leaderboard --features loadtest \
//!     --bin gen_corpus -- --out loadtest/corpus --submits 200

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use encrypted_leaderboard::loadtest_support::{enc_id, enc_score, keys};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Serialize;
use serde_json::json;
use tracing::{info, info_span};

/// CLI für den Corpus-Generator. Argumente sind so gewählt, dass dieselben
/// Werte wie im Default-Loadtest ohne explizite Flags rauskommen.
#[derive(Parser, Debug)]
#[command(about = "Generiert den FHE-Corpus für k6-Loadtests")]
struct Cli {
    /// Zielverzeichnis. Wird angelegt falls nicht vorhanden.
    #[arg(long, default_value = "loadtest/corpus")]
    out: PathBuf,

    /// Anzahl der vor-verschlüsselten Submit-Payloads.
    /// Mehr Payloads = mehr Variation in den k6-VUs, aber längere Generierung.
    #[arg(long, default_value_t = 200)]
    submits: usize,

    /// Anzahl unterschiedlicher Spieler-Keys, die zyklisch wiederverwendet werden.
    /// `submits / players` ergibt die durchschnittliche Re-Submit-Häufigkeit pro Spieler
    /// (z.B. 200 / 20 = 10× pro Spieler → viele FHE-`keep_max`-Operationen im Test).
    #[arg(long, default_value_t = 20)]
    players: usize,

    /// Seed für reproduzierbare Score-Werte. Gleicher Seed → bitweise identischer Corpus.
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

/// Inhalt der `meta.json`. Wird vom Spec-Reporting referenziert, um die
/// Messung später nachstellen zu können.
#[derive(Serialize)]
struct Meta {
    tfhe_version: &'static str,
    tfhe_config: &'static str,
    score_type: &'static str,
    id_type: &'static str,
    submits: usize,
    players: usize,
    seed: u64,
    generated_at_utc: String,
    git_sha: String,
    server_key_b64_bytes: usize,
}

fn main() -> Result<()> {
    // Minimaler Tracing-Subscriber: Default-Level INFO, überschreibbar mit
    // `RUST_LOG=debug` falls jemand mehr Detail will.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();
    fs::create_dir_all(&cli.out)
        .with_context(|| format!("Konnte Output-Verzeichnis {:?} nicht anlegen", cli.out))?;

    // Schritt 1: FHE-Schlüsselpaar erzeugen.
    // Erste Keys()-Aufruf triggert die teure Initialisierung (~30–60 s).
    let t = Instant::now();
    info!("Erzeuge TFHE-Schlüsselpaar (kann ~30–60 s dauern) …");
    let test_keys = {
        let _span = info_span!("keygen").entered();
        keys()
    };
    let server_key_b64_bytes = test_keys.server_key_b64.len();
    info!(
        elapsed_secs = t.elapsed().as_secs(),
        server_key_b64_bytes, "Schlüssel bereit"
    );

    // Schritt 2: Schlüssel-Dateien schreiben.
    write_text(
        cli.out.join("server_key.b64"),
        &test_keys.server_key_b64,
        "server_key.b64",
    )?;

    // Der Service akzeptiert beliebige base64-Daten im `public_key`-Feld und
    // gibt sie unverändert an Spieler weiter — er nutzt sie selbst nie. Für den
    // Loadtest reicht daher ein Platzhalter; alle Ciphertexte werden mit dem
    // ClientKey verschlüsselt, nicht mit dem PublicKey.
    let public_key_b64 = "AAAA";
    write_text(
        cli.out.join("public_key.b64"),
        public_key_b64,
        "public_key.b64",
    )?;

    let client_key_bytes = bincode::serialize(&test_keys.client_key)
        .context("ClientKey-Serialisierung fehlgeschlagen")?;
    write_bytes(
        cli.out.join("client_key.bin"),
        &client_key_bytes,
        "client_key.bin",
    )?;

    // Schritt 3: `create_body.json` — direkt für k6 als Request-Body verwendbar.
    let create_body = json!({
        "server_key": test_keys.server_key_b64,
        "public_key": public_key_b64,
    });
    let create_body_str = serde_json::to_string(&create_body)?;
    write_text(
        cli.out.join("create_body.json"),
        &create_body_str,
        "create_body.json",
    )?;

    // Schritt 4: Submit-Payloads als NDJSON streamen.
    // Wir schreiben jeden Eintrag sofort weg, damit der Heap auch bei großen
    // `--submits` nicht volläuft (ein Ciphertext-Paar ist mehrere KB groß).
    info!(
        count = cli.submits,
        players = cli.players,
        "Verschlüssele Submit-Payloads"
    );
    let payloads_path = cli.out.join("submit_payloads.ndjson");
    let mut writer = BufWriter::new(
        File::create(&payloads_path)
            .with_context(|| format!("Konnte {payloads_path:?} nicht anlegen"))?,
    );

    let mut rng = StdRng::seed_from_u64(cli.seed);
    let t_encrypt = Instant::now();
    for i in 0..cli.submits {
        let player_idx = i % cli.players;
        let player_key = format!("player_{player_idx}");
        let score: u16 = rng.gen();
        // ID innerhalb des Rooms ist stabil pro player_idx — sonst würde sich
        // bei Re-Submits die ID ändern, was die `keep_max`-Semantik des Service
        // verletzt (Service prüft player_key für Dedup, ID ist nur Anzeigewert).
        let id_val = (player_idx % 256) as u8;

        let encrypted_score = enc_score(score);
        let encrypted_id = enc_id(id_val);

        let line = json!({
            "player_key": player_key,
            "encrypted_score": encrypted_score,
            "encrypted_id": encrypted_id,
        });
        writeln!(writer, "{line}")?;

        if (i + 1) % 20 == 0 || i + 1 == cli.submits {
            info!(
                done = i + 1,
                total = cli.submits,
                elapsed_secs = t_encrypt.elapsed().as_secs(),
                "Encrypt-Fortschritt"
            );
        }
    }
    writer.flush()?;
    info!(
        path = %payloads_path.display(),
        elapsed_secs = t_encrypt.elapsed().as_secs(),
        "Submit-Payloads geschrieben"
    );

    // Schritt 5: Meta-Datei für Reproduzierbarkeit.
    let meta = Meta {
        tfhe_version: "1.6.1",
        tfhe_config: "ConfigBuilder::default()",
        score_type: "FheUint16",
        id_type: "FheUint8",
        submits: cli.submits,
        players: cli.players,
        seed: cli.seed,
        generated_at_utc: now_utc_iso(),
        git_sha: git_sha(),
        server_key_b64_bytes,
    };
    write_text(
        cli.out.join("meta.json"),
        &serde_json::to_string_pretty(&meta)?,
        "meta.json",
    )?;

    info!(
        out = %cli.out.display(),
        total_secs = t.elapsed().as_secs(),
        "Corpus fertig"
    );
    Ok(())
}

/// Schreibt eine UTF-8-Datei und loggt den Pfad + Größe.
fn write_text(path: PathBuf, content: &str, label: &str) -> Result<()> {
    fs::write(&path, content).with_context(|| format!("write {label} → {path:?}"))?;
    info!(path = %path.display(), bytes = content.len(), "{label}");
    Ok(())
}

/// Variante für Binär-Inhalte (z.B. `client_key.bin`).
fn write_bytes(path: PathBuf, content: &[u8], label: &str) -> Result<()> {
    fs::write(&path, content).with_context(|| format!("write {label} → {path:?}"))?;
    info!(path = %path.display(), bytes = content.len(), "{label}");
    Ok(())
}

/// Aktuelles UTC-Datum als ISO-8601-String über die `time`-Crate.
fn now_utc_iso() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Iso8601::DEFAULT)
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Versucht den aktuellen Git-Commit-Hash zu lesen. Bei Fehlern (kein Git,
/// nicht im Repo, …) geben wir `"unknown"` zurück — der Corpus bleibt brauchbar.
fn git_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
