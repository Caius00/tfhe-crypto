use std::sync::Arc;
use redis::{AsyncCommands, Client};

pub struct AppState {
    client: Client,
}

// get state from router TODO(huh?)
pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = Client::open(redis_url)?;
        Ok(Self {client})
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


















