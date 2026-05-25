use std::sync::Arc;
use redis::{AsyncCommands, Client};
use tfhe::{ClientKey, FheAsciiString, FheBool};
use tfhe::prelude::{FheEq, IfThenElse};
use std::error::Error;
use std::ops::BitOr;
use rayon::prelude::*;
use crate::custom_fhe_ascii_string::CustomFheAsciiString;

pub struct AppState {
    pub client: Client,
    pub ttl_sec: u64,
}

// get state from router
pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(redis_url: &str, cks: &ClientKey, ttl_sec: u64) -> Result<Self, Box<dyn Error>>  {
        let client = Client::open(redis_url)?;
        let mut con = client.get_connection()?;

        let pong: String = redis::cmd("PING").query(&mut con)?;
        println!("{pong}. DB alive");

        Ok(Self {client, ttl_sec})
    }

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
