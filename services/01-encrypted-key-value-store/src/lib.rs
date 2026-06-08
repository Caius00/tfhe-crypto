//! Encrypted Key-Value Store — Crate-Wurzel.
//!
//! Architektur-Überblick:
//! - `codec`                  : Base64-Helfer für den HTTP-Transport von Ciphertexts
//! - `custom_fhe_ascii_string`: Zeichenweise verschlüsselte Strings (FheUint8-Vektor)
//! - `models`                 : Request-/Response-Schemas + `AppError`
//! - `store`                  : Redis-IO + ServerKey-Map pro Session
//! - `routes`                 : Axum-Handler — kombinieren codec, store und FHE-Logik

pub mod codec;
pub mod custom_fhe_ascii_string;
pub mod models;
pub mod routes;
pub mod store;
