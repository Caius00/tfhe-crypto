use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tfhe::prelude::*;
use tfhe::{
    generate_keys, ClientKey, ConfigBuilder, FheBool, FheInt32, FheInt64, FheUint16, FheUint32,
    FheUint64, FheUint8, ServerKey,
};

/// Which TFHE integer type to use for encryption.
/// Smaller types encrypt faster; use the smallest type that fits your value range.
#[derive(Debug, Clone, PartialEq)]
pub enum FheType {
    /// Unsigned 8-bit  (0 – 255)           Fast. Good for pixels, DNA bases.
    UInt8,
    /// Unsigned 16-bit (0 – 65 535)         Good for small counters.
    UInt16,
    /// Unsigned 32-bit (0 – 4 294 967 295)  Good for ages, scores, votes.
    UInt32,
    /// Unsigned 64-bit (0 – 2^64-1)         General purpose.
    UInt64,
    /// Signed 32-bit   (±2 147 483 647)     When negative values are possible.
    Int32,
    /// Signed 64-bit   (±2^63-1)            Large signed values.
    Int64,
    /// Boolean         (0 or 1)             Single bit operations.
    Bool,
}

impl FheType {
    pub fn label(&self) -> &'static str {
        match self {
            FheType::UInt8 => "u8  (0..255)",
            FheType::UInt16 => "u16 (0..65535)",
            FheType::UInt32 => "u32 (0..4B)",
            FheType::UInt64 => "u64 (large)",
            FheType::Int32 => "i32 (±2B)",
            FheType::Int64 => "i64 (±large)",
            FheType::Bool => "bool (0/1)",
        }
    }

    pub fn all() -> &'static [FheType] {
        &[
            FheType::UInt8,
            FheType::UInt16,
            FheType::UInt32,
            FheType::UInt64,
            FheType::Int32,
            FheType::Int64,
            FheType::Bool,
        ]
    }

    pub fn supports_value(&self, raw: &str) -> bool {
        match self {
            FheType::UInt8 => raw.parse::<u8>().is_ok(),
            FheType::UInt16 => raw.parse::<u16>().is_ok(),
            FheType::UInt32 => raw.parse::<u32>().is_ok(),
            FheType::UInt64 => raw.parse::<u64>().is_ok(),
            FheType::Int32 => raw.parse::<i32>().is_ok(),
            FheType::Int64 => raw.parse::<i64>().is_ok(),
            FheType::Bool => matches!(raw.trim(), "0" | "1" | "true" | "false"),
        }
    }
}

// ── TFHE client ───────────────────────────────────────────────────────────────

pub struct TfheClient {
    client_key: ClientKey,
    server_key: ServerKey,
}

impl TfheClient {
    pub fn generate() -> Self {
        let config = ConfigBuilder::default().build();
        let (client_key, server_key) = generate_keys(config);
        TfheClient {
            client_key,
            server_key,
        }
    }

    /// Export the server key as base64.
    /// This must be sent to the server so it can perform homomorphic operations.
    /// The server key is NOT secret — it cannot decrypt anything.
    pub fn server_key_b64(&self) -> Result<String> {
        let bytes =
            bincode::serialize(&self.server_key).context("Failed to serialize server key")?;
        Ok(BASE64.encode(bytes))
    }

    /// Encrypt a value using the given TFHE type.
    /// Returns a base64-encoded ciphertext.
    pub fn encrypt(&self, raw: &str, fhe_type: &FheType) -> Result<String> {
        let bytes = match fhe_type {
            FheType::UInt8 => {
                let v: u8 = raw.trim().parse().context("Expected u8 (0..255)")?;
                bincode::serialize(&FheUint8::encrypt(v, &self.client_key))
            }
            FheType::UInt16 => {
                let v: u16 = raw.trim().parse().context("Expected u16 (0..65535)")?;
                bincode::serialize(&FheUint16::encrypt(v, &self.client_key))
            }
            FheType::UInt32 => {
                let v: u32 = raw.trim().parse().context("Expected u32 (0..4294967295)")?;
                bincode::serialize(&FheUint32::encrypt(v, &self.client_key))
            }
            FheType::UInt64 => {
                let v: u64 = raw.trim().parse().context("Expected u64")?;
                bincode::serialize(&FheUint64::encrypt(v, &self.client_key))
            }
            FheType::Int32 => {
                let v: i32 = raw.trim().parse().context("Expected i32 (±2147483647)")?;
                bincode::serialize(&FheInt32::encrypt(v, &self.client_key))
            }
            FheType::Int64 => {
                let v: i64 = raw.trim().parse().context("Expected i64")?;
                bincode::serialize(&FheInt64::encrypt(v, &self.client_key))
            }
            FheType::Bool => {
                let v: bool = match raw.trim() {
                    "1" | "true" => true,
                    "0" | "false" => false,
                    _ => anyhow::bail!("Expected bool: 0, 1, true, or false"),
                };
                bincode::serialize(&FheBool::encrypt(v, &self.client_key))
            }
        }
        .context("Serialization failed")?;

        Ok(BASE64.encode(bytes))
    }

    /// Decrypt a base64 ciphertext using the given type.
    pub fn decrypt(&self, b64: &str, fhe_type: &FheType) -> Result<String> {
        let bytes = BASE64.decode(b64).context("Invalid base64")?;

        let result = match fhe_type {
            FheType::UInt8 => {
                let ct: FheUint8 = bincode::deserialize(&bytes).context("Deserialize failed")?;
                let v: u8 = ct.decrypt(&self.client_key);
                v.to_string()
            }
            FheType::UInt16 => {
                let ct: FheUint16 = bincode::deserialize(&bytes).context("Deserialize failed")?;
                let v: u16 = ct.decrypt(&self.client_key);
                v.to_string()
            }
            FheType::UInt32 => {
                let ct: FheUint32 = bincode::deserialize(&bytes).context("Deserialize failed")?;
                let v: u32 = ct.decrypt(&self.client_key);
                v.to_string()
            }
            FheType::UInt64 => {
                let ct: FheUint64 = bincode::deserialize(&bytes).context("Deserialize failed")?;
                let v: u64 = ct.decrypt(&self.client_key);
                v.to_string()
            }
            FheType::Int32 => {
                let ct: FheInt32 = bincode::deserialize(&bytes).context("Deserialize failed")?;
                let v: i32 = ct.decrypt(&self.client_key);
                v.to_string()
            }
            FheType::Int64 => {
                let ct: FheInt64 = bincode::deserialize(&bytes).context("Deserialize failed")?;
                let v: i64 = ct.decrypt(&self.client_key);
                v.to_string()
            }
            FheType::Bool => {
                let ct: FheBool = bincode::deserialize(&bytes).context("Deserialize failed")?;
                let v: bool = ct.decrypt(&self.client_key);
                v.to_string()
            }
        };

        Ok(result)
    }
}
