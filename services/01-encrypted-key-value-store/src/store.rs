use std::sync::Arc;
use redis::{AsyncCommands, Client};
use tfhe::{ClientKey, FheAsciiString, FheBool, FheUint8};
use tfhe::prelude::{FheEq, FheStringMatching, FheTryEncrypt, IfThenElse};
use std::error::Error;
use std::ops::Not;
use serde::{Deserialize, Serialize};

struct CustomFheAsciiString(FheAsciiString);
impl CustomFheAsciiString {
    /// Custom if_then_else implementation for FheAsciiString
    pub fn if_then_else_string(
        condition: &FheBool,
        then_value: &FheAsciiString,
        else_value: &FheAsciiString,
    ) -> Result<FheAsciiString, Box<dyn Error>> {
        // TODO() enforce equal length/ handle length

        let mut result_bytes = Vec::new();

        let then_bytes = CustomFheAsciiString::fhe_string_to_bytes(then_value)?;
        let else_bytes = CustomFheAsciiString::fhe_string_to_bytes(else_value)?;
        let then_iter = then_bytes.iter();
        let else_iter = else_bytes.iter();

        // Perform conditional selection byte-by-byte
        for (then_byte, else_byte) in then_iter.zip(else_iter) {
            // Use existing if_then_else for numeric types (FheUint8)
            let selected_byte = condition.if_then_else(then_byte, else_byte);
            result_bytes.push(selected_byte);
        }

        // Reconstruct FheAsciiString from selected bytes
        let result = CustomFheAsciiString::bytes_to_fhe_string(&*result_bytes)?;

        Ok(result)
    }

    // Convert FheAsciiString to Vec<FheUint8>
    pub fn fhe_string_to_bytes(fhe_string: &FheAsciiString) -> Result<Vec<FheUint8>, Box<dyn Error>> {
        let serialized = bincode::serialize(fhe_string)?;
        let bytes_vec: Vec<FheUint8> = bincode::deserialize(&serialized)?;
        Ok(bytes_vec)
    }

    // Convert Vec<FheUint8> back to FheAsciiString
    pub fn bytes_to_fhe_string(bytes: &[FheUint8]) -> Result<FheAsciiString, Box<dyn Error>> {
        let serialized = bincode::serialize(bytes)?;
        let fhe_string: FheAsciiString = bincode::deserialize(&serialized)?;
        Ok(fhe_string)
    }
}

pub struct AppState {
    client: Client,
    default_value: FheAsciiString,
    ttl_sec: u64,
}

// get state from router
pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(redis_url: &str, cks: &ClientKey, ttl_sec: u64) -> Result<Self, Box<dyn Error>>  {
        let default_value = FheAsciiString::try_encrypt("Hello World", cks)?;
        let client = Client::open(redis_url)?;
        Ok(Self {client, default_value, ttl_sec})
    }
    fn user_key(user_hash: &str, key: &str) -> String {
        format!("user:{}:{}", user_hash, key)
    }

    pub async fn set(
        &self,
        user_hash: &str,
        key: &str,
        value: &str,
        ttl_sec: u64
    ) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let formatted_key = Self::user_key(user_hash, key);
        conn.set_ex(&formatted_key, value, ttl_sec).await
    }

    // TODO() users can choose same user_name
    /// key is "user:{user_name}:{key}"
    pub async fn set_enc(
        &self,
        key: &FheAsciiString,
        value: &FheAsciiString,
    ) -> Result<(), Box<dyn Error>> {
        let ttl = self.ttl_sec.clone();
        let mut conn = self.client.get_multiplexed_async_connection().await?;

        let serialize_key = bincode::serialize(key)?;
        let serialize_value = bincode::serialize(value)?;
        conn.set_ex::<_, _, ()>(serialize_key, serialize_value, ttl).await?;

        Ok(())
    }

    /// key is "user:{user_name}:{key}"
    pub async fn get_enc(
        &self,
        key: &FheAsciiString,
    ) -> Result<FheAsciiString, Box<dyn Error>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let mut cursor: u64 = 0;
        let mut result = self.default_value.clone();

        loop {
            let (next, keys): (u64, Vec<Vec<u8>>) =
                redis::cmd("SCAN")
                    .cursor_arg(cursor)
                    .query_async(&mut conn)
                    .await?;

            cursor = next;

            let values: Vec<Option<Vec<u8>>> = conn.mget(&keys).await?;

            for (found_key, value_opt) in keys.iter().zip(values.iter()) {
                if let Some(value) = value_opt {
                    let deserialize_key = bincode::deserialize::<FheAsciiString>(&*found_key)?;
                    let deserialize_value = bincode::deserialize::<FheAsciiString>(&*value)?;

                    let is_match = deserialize_key.eq(key);
                    result = CustomFheAsciiString::if_then_else_string(
                        &is_match,
                        &deserialize_value,
                        &result,
                    )?;
                }
            }

            if cursor == 0 {
                break;
            }
        }

        Ok(result)
    }

    // not suing this since it loads all keys in mem
    // pub async fn get_enc(
    //     &self,
    //     key: &FheAsciiString,
    // ) -> Result<FheAsciiString, Box<dyn Error>> {
    //     let mut conn = self.client.get_multiplexed_async_connection().await?;
    //     let mut iter: redis::AsyncIter<Vec<u8>> = conn.scan().await?;
    //
    //     let mut keys = Vec::new();
    //     while let Some(found_key) = iter.next_item().await {
    //         keys.push(found_key);
    //     }
    //     drop(iter)
    //     let values: Vec<Option<Vec<u8>>> = conn.mget(&keys).await?;
    //
    //     let mut result = self.default_value.clone();
    //     for (found_key, value_opt) in keys.iter().zip(values.iter()) {
    //         if let Some(value) = value_opt {
    //             let deserialize_key = bincode::deserialize::<FheAsciiString>(&*found_key)?;
    //             let deserialize_value = bincode::deserialize::<FheAsciiString>(&*value)?;
    //
    //             let is_match = deserialize_key.eq(key);
    //             result = CustomFheAsciiString::if_then_else_string(
    //                 &is_match,
    //                 &deserialize_value,
    //                 &result,
    //             )?;
    //         }
    //     }
    //
    //     Ok(result)
    // }

    // TODO() use is_match from get_enc instead?
    /// key is "user:{user_name}:{key}"
    pub async fn exists_enc(
        &self,
        key: &FheAsciiString,
    ) -> Result<FheBool, Box<dyn Error>> {
        let potential_value = self.get_enc(key).await?;
        let default = self.default_value.clone();
        let found_default = potential_value.eq(&default);

        Ok(found_default.not())
    }

    /// identifier is "user:{user_name}"
    pub async fn clear_enc(
        &self,
        identifier: &FheAsciiString,
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let mut cursor: u64 = 0;

        loop {
            let (next, keys): (u64, Vec<Vec<u8>>) =
                redis::cmd("SCAN")
                    .cursor_arg(cursor)
                    .query_async(&mut conn)
                    .await?;

            cursor = next;

            for found_key in keys.iter() {
                // check if key eq key but subset
                // user:{user_name}:{key}
                let deserialize_key = bincode::deserialize::<FheAsciiString>(&*found_key)?;
                let is_match = deserialize_key.starts_with(identifier);
            }

            if cursor == 0 {
                break;
            }
        }

        Ok(())
    }

    pub async fn get(
        &self,
        user_hash: &str,
        key: &str
    ) -> Result<Option<String>, redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let formatted_key = Self::user_key(user_hash, key);
        conn.get(&formatted_key).await
    }

    pub async fn delete(
        &self,
        user_hash: &str,
        key: &str
    ) -> Result<bool, redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let formatted_key = Self::user_key(user_hash, key);

        Ok(conn.del(&formatted_key).await?)
    }

    pub async fn exists(
        &self,
        user_hash: &str,
        key: &str
    ) -> Result<bool, redis::RedisError> {
        let value_found = Self::get(self, user_hash, key).await?;

        Ok(value_found.is_some())
    }

    pub async fn list_entries(
        &self,
        user_hash: &str
    ) -> Result<Vec<String>, redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let pattern = format!("user:{}:*", user_hash);
        let keys: Vec<String> = conn.keys(pattern).await?;
        let prefix = format!("user:{}:", user_hash);

        Ok(keys
            .iter()
            .filter_map(|k| k.strip_prefix(&prefix))
            .map(|s| s.to_string())
            .collect()
        )
    }

    pub async fn delete_all(
        &self,
        user_hash: &str
    ) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::list_entries(self, user_hash).await?;

        if !keys.is_empty() {
            conn.del(&keys).await?
        }
        Ok(())
    }
}


















