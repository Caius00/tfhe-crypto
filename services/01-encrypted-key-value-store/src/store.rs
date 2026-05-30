use std::env;
use std::sync::Arc;
use redis::{AsyncCommands, Client, ConnectionAddr, ConnectionInfo, RedisConnectionInfo};
use tfhe::{FheBool};
use tfhe::prelude::{FheEq, IfThenElse};
use std::error::Error;
use std::ops::BitOr;
use dotenvy::from_path;
use rayon::prelude::*;
use crate::custom_fhe_ascii_string::CustomFheAsciiString;

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
            password: if password.is_empty() { None } else { Some(password) },
        },
    };

    Client::open(conn_info).expect("Failed to connect to Redis")
}

pub struct AppState {
    pub client: Client,
    pub ttl_sec: u64,
}

// get state from router
pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new() -> Self {
        if let Err(e) = from_path("./services/01-encrypted-key-value-store/.env") {
            eprintln!("Warning: could not load .env: {}. Falling back to localhost defaults.", e);
        }

        let ttl_minutes = env::var("TTL_MINUTES")
            .unwrap_or_else(|_| "5".to_string())
            .parse()
            .unwrap_or(5);
        let ttl_sec = ttl_minutes * 60;
        let client = get_redis_client();

        Self {client, ttl_sec}
    }

    // Possibly dont take serialized arguments
    pub async fn put(
        &self,
        key: &CustomFheAsciiString,
        value: &CustomFheAsciiString,
    ) {
        let mut con = self
            .client
            .get_multiplexed_async_connection()
            .await
            .expect("Error connecting to DB");

        let serialized_key = key.serialize().string;
        let serialized_value = value.serialize().string;

        con
            .set_ex::<Vec<u8>, Vec<u8>, ()>(serialized_key, serialized_value, self.ttl_sec)
            .await
            .expect("Error inserting into DB");
    }

    pub async fn get(
        &self,
        key: &CustomFheAsciiString,
    ) -> Result<(CustomFheAsciiString, FheBool), Box<dyn Error>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let mut iter: redis::AsyncIter<Vec<u8>> = con.scan().await?;

        let mut keys = Vec::new();
        while let Some(found_key) = iter.next_item().await {
            keys.push(found_key);
        }
        drop(iter);

        let values: Vec<Option<Vec<u8>>> = con.mget(&keys).await?;

        println!("Setup finished. Trying to get value now!");
        let (is_match, last_found_value) = keys
            .iter()
            .zip(values.iter())
            .filter_map(
                |(k, v_opt)| {
                    v_opt.as_ref().map(|v| {
                        (
                            CustomFheAsciiString::from(k),
                            CustomFheAsciiString::from(v),
                        )
                })
            })
            .map(|(found_key, found_value)| (found_key.eq(key.clone()), found_value))
            .reduce(|(acc_match, acc_value), (is_match, found_value)| {
                let new_match = acc_match.bitor(is_match);
                let next_found_value = new_match.if_then_else(&found_value, &acc_value);
                (new_match, next_found_value)
            })
            .unwrap();
        println!("Got value!");

        Ok((last_found_value, is_match))
    }

    pub async fn exists(
        &self,
        key: &CustomFheAsciiString,
    ) -> Result<FheBool, Box<dyn Error>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let mut iter: redis::AsyncIter<Vec<u8>> = con.scan().await?;

        let mut keys = Vec::new();
        while let Some(found_key) = iter.next_item().await {
            keys.push(found_key);
        }
        drop(iter);

        println!("Setup finished. Trying to get value now!");
        let is_match = keys
            .iter()
            .map(|k| {
                let found_key  = CustomFheAsciiString::from(k);
                found_key.eq(key.clone())
            })
            .reduce(|acc, is_match| {
                acc.bitor(is_match)
            })
            .unwrap();

        Ok(is_match)
    }

    /// dont use. should just wait for entries to expire
    pub async fn delete(
        &self,
        key: &CustomFheAsciiString,
    ) -> Result<(), Box<dyn Error>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let serialized_key = key.serialize().string;

        con.del::<_, ()>(serialized_key).await?;

        Ok(())
    }

    /// dont use. should just wait for entries to expire
    pub async fn delete_multiple(
        &self,
        keys: &Vec<CustomFheAsciiString>,
    ) -> Result<(), Box<dyn Error>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let serialized_keys = keys
            .par_iter()
            .map(|k| {
                k.serialize().string
            })
            .collect::<Vec<Vec<u8>>>();

        con.del::<_, ()>(serialized_keys).await?;

        Ok(())
    }
}
