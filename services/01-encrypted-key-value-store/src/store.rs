use crate::custom_fhe_ascii_string::{CompressedCustomFheAsciiString, CustomFheAsciiString};
use crate::models::AppError;
use dotenvy::from_path;
use redis::{AsyncCommands, Client, ConnectionAddr, ConnectionInfo, RedisConnectionInfo};
use std::collections::HashMap;
use std::ops::BitOr;
use std::sync::Arc;
use std::{env, thread};
use tfhe::prelude::{FheEq, FheTrivialEncrypt, IfThenElse};
use tfhe::FheBool;
use tfhe::ServerKey;
use tokio::sync::RwLock;

fn get_redis_client() -> Client {
    let password = env::var("REDIS_PASSWORD").unwrap_or_default();
    let host = env::var("REDIS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

    let port = env::var("REDIS_PORT")
        .unwrap_or_else(|_| "6379".to_string())
        .parse()
        .unwrap_or(6379);

    let conn_info = ConnectionInfo {
        addr: ConnectionAddr::Tcp(host, port),
        redis: RedisConnectionInfo {
            db: 0,
            username: None,
            password: if password.is_empty() {
                None
            } else {
                Some(password)
            },
        },
    };

    Client::open(conn_info).expect("Failed to connect to Redis")
}

pub struct AppState {
    pub client: Client,
    pub ttl_sec: u64,
    pub server_keys: RwLock<HashMap<String, ServerKey>>,
}

// get state from router
pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new() -> Self {
        if let Err(e) = from_path("./services/01-encrypted-key-value-store/.env") {
            eprintln!(
                "Warning: could not load .env: {}. Falling back to localhost defaults.",
                e
            );
        }

        let ttl_minutes = env::var("TTL_MINUTES")
            .unwrap_or_else(|_| "5".to_string())
            .parse()
            .unwrap_or(5);
        let ttl_sec = ttl_minutes * 60;
        let client = get_redis_client();
        let server_keys = RwLock::new(HashMap::new());

        Self {
            client,
            ttl_sec,
            server_keys,
        }
    }

    pub async fn put(
        &self,
        key: &CustomFheAsciiString,
        value: &CustomFheAsciiString,
        session_id: &str,
    ) -> Result<(), AppError> {
        println!("Putting in thread: {:?}", thread::current().id());
        let mut con = self.client.get_multiplexed_async_connection().await?;

        let db_key = Self::create_db_key(key, session_id);
        let compressed_value = value.compress().string;

        con.set_ex::<Vec<u8>, Vec<u8>, ()>(db_key, compressed_value, self.ttl_sec)
            .await?;

        Ok(())
    }

    pub async fn get(
        &self,
        key: &CustomFheAsciiString,
        session_id: &str,
    ) -> Result<(CustomFheAsciiString, FheBool), AppError> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let mut iter: redis::AsyncIter<Vec<u8>> = con.scan().await?;

        let mut keys = Vec::new();
        while let Some(found_key) = iter.next_item().await {
            keys.push(found_key);
        }
        drop(iter);

        let values: Vec<Option<Vec<u8>>> = con.mget(&keys).await?;

        let (is_match, last_found_value) = keys
            .into_iter()
            .zip(values.into_iter())
            .filter_map(|(k, v_opt)| {
                let v = v_opt?;

                let (sid, found_key) = Self::parse_db_key(&k).ok()?;
                if sid != session_id {
                    return None;
                } else {
                    Some((
                        found_key.eq(key.clone()),
                        CompressedCustomFheAsciiString::new(v).decompress(),
                    ))
                }
            })
            .fold(None::<(FheBool, CustomFheAsciiString)>, |acc, (m, v)| {
                Some(match acc {
                    None => (m.clone(), v),
                    Some((acc_m, acc_v)) => {
                        let new_m = acc_m.bitor(m);
                        let new_v = new_m.if_then_else(&v, &acc_v);
                        (new_m, new_v)
                    }
                })
            })
            .ok_or_else(|| AppError::NotFound(session_id.to_string()))?;

        Ok((last_found_value, is_match))
    }

    pub async fn exists(
        &self,
        key: &CustomFheAsciiString,
        session_id: &str,
    ) -> Result<FheBool, AppError> {
        println!("Exists in thread: {:?}", thread::current().id());
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let mut iter: redis::AsyncIter<Vec<u8>> = con.scan().await?;

        let mut keys = Vec::new();
        while let Some(found_key) = iter.next_item().await {
            keys.push(found_key);
        }
        drop(iter);

        let mut result = FheBool::encrypt_trivial(false);
        for k in keys {
            let Ok((sid, found_key)) = Self::parse_db_key(&k) else {
                continue;
            };
            if sid != session_id {
                continue;
            } else {
                println!("Found match for session: {}", session_id);
                result = result.bitor(found_key.eq(key.clone()));
            }
        }

        Ok(result)
    }

    /// dont use. should just wait for entries to expire
    pub async fn delete(
        &self,
        key: &CustomFheAsciiString,
        session_id: &str,
    ) -> Result<(), AppError> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let db_key = Self::create_db_key(key, session_id);

        con.del::<_, ()>(db_key).await?;

        Ok(())
    }

    fn create_db_key(key: &CustomFheAsciiString, session_id: &str) -> Vec<u8> {
        let mut db_key = Vec::new();

        let len = session_id.len() as u32;
        db_key.extend_from_slice(&len.to_be_bytes());
        db_key.extend_from_slice(session_id.as_bytes());
        db_key.extend_from_slice(&key.compress().string);

        db_key
    }

    fn parse_db_key(db_key: &[u8]) -> Result<(String, CustomFheAsciiString), AppError> {
        let len_bytes: [u8; 4] = db_key
            .get(0..4)
            .ok_or_else(|| AppError::InternalError("Corrupted DB key".to_string()))?
            .try_into()
            .map_err(|_| AppError::InternalError("Corrupted DB key".to_string()))?;

        let len = usize::try_from(u32::from_be_bytes(len_bytes))
            .map_err(|_| AppError::InternalError("Corrupted DB key".to_string()))?;

        let session_bytes = db_key
            .get(4..4 + len)
            .ok_or_else(|| AppError::InternalError("Corrupted DB key".to_string()))?;

        let session_id = String::from_utf8(session_bytes.to_vec())
            .map_err(|_| AppError::InternalError("Corrupted DB key".to_string()))?;

        let key_bytes = db_key
            .get(4 + len..)
            .ok_or_else(|| AppError::InternalError("Corrupted DB key".to_string()))?;

        let key = CompressedCustomFheAsciiString::new(key_bytes.to_vec());

        Ok((session_id, key.decompress()))
    }
}

#[cfg(test)]
mod test_exists {
    use super::*;
    use redis::Commands;
    use std::time::Duration;
    use tfhe::prelude::FheDecrypt;
    use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
    use tfhe::{set_server_key, ClientKey, CompressedServerKey};
    use tokio::time::sleep;
    use uuid::Uuid;

    async fn run_basic(app_state: Arc<AppState>, key: &str, value: &str) {
        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);
        let session_id = Uuid::new_v4().to_string();
        let db_key = AppState::create_db_key(&enc_key, &session_id);

        let enc_should_false = app_state.exists(&enc_key, &session_id).await.unwrap();

        let mut con = app_state.client.get_connection().unwrap();
        con.set::<Vec<u8>, Vec<u8>, ()>(db_key, enc_value.compress().string)
            .unwrap();

        let enc_should_true = app_state.exists(&enc_key, &session_id).await.unwrap();

        let should_false = enc_should_false.decrypt(&client_key);
        let should_true = enc_should_true.decrypt(&client_key);

        assert!(!should_false);
        assert!(should_true);
    }

    #[tokio::test]
    async fn basic() {
        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let enc_key = CustomFheAsciiString::new("Hello Key", &client_key);
        let enc_value = CustomFheAsciiString::new("Hello Value", &client_key);
        let session_id = Uuid::new_v4().to_string();
        let db_key = AppState::create_db_key(&enc_key, &session_id);

        let app_state = AppState::new();

        let enc_should_false = app_state.exists(&enc_key, &session_id).await.unwrap();

        let mut con = app_state.client.get_connection().unwrap();
        con.set::<Vec<u8>, Vec<u8>, ()>(db_key, enc_value.compress().string)
            .unwrap();

        let enc_should_true = app_state.exists(&enc_key, &session_id).await.unwrap();

        let should_false = enc_should_false.decrypt(&client_key);
        let should_true = enc_should_true.decrypt(&client_key);

        assert!(!should_false);
        assert!(should_true);
    }

    #[tokio::test]
    async fn basic_threads() {
        let app_state = Arc::new(AppState::new());

        let a1 = Arc::clone(&app_state);
        let a2 = Arc::clone(&app_state);

        let c1 = tokio::spawn(run_basic(a1, "Hello Key A", "Hello Value A"));
        // sleep(Duration::from_secs(30)).await;
        let c2 = tokio::spawn(run_basic(a2, "Hello Key B", "Hello Value B"));

        let results = tokio::try_join!(c1, c2).unwrap();
    }
}
