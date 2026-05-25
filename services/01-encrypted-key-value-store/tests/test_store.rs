use tfhe::{generate_keys, set_server_key, ConfigBuilder, FheAsciiString, ServerKey};
use tfhe::prelude::{FheDecrypt, FheTryEncrypt};
use encrypted_key_value_store::custom_fhe_ascii_string::CustomFheAsciiString;
use encrypted_key_value_store::store::AppState;

async fn setup() -> (AppState, ServerKey, CustomFheAsciiString, CustomFheAsciiString) {
    let ttl_sec = 60u64;
    let config = ConfigBuilder::default().build();
    let (client_key, server_key) = generate_keys(config);

    const REDIS_URL: &str = "redis://localhost:6379";
    let app_state = AppState::new(REDIS_URL, &client_key, ttl_sec).expect("Failed to connect to Redis.");

    let key = CustomFheAsciiString::new("Hello Key", &client_key);
    let value = CustomFheAsciiString::new("Hello Value", &client_key);

    (app_state, server_key, key, value)
}

#[cfg(test)]
mod test_set{
    use redis::Commands;
    use super::*;

    #[tokio::test]
    async fn single_insert() {
        // CLIENT
        let ttl_sec = 60u64;
        let config = ConfigBuilder::default().build();
        let (client_key, server_key) = generate_keys(config);
        const REDIS_URL: &str = "redis://localhost:6379";
        let app_state = AppState::new(REDIS_URL, &client_key, ttl_sec).expect("Failed to connect to Redis.");
        let mut con = app_state.client.get_connection().unwrap();
        redis::cmd("FLUSHDB").query::<()>(&mut con).unwrap();

        let key = "Hello Key";
        let value = "Hello Value";
        let mut enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);

        // SERVER
        let server = tokio::spawn(async move {
            set_server_key(server_key);
            app_state.put(&enc_key, &enc_value).await;
            enc_key
        });
        // CLIENT
        enc_key = server.await.unwrap();
        let db_size: usize = redis::cmd("DBSIZE").query(&mut con).unwrap();
        let entry: Vec<u8> = con.get(enc_key.serialize().string).unwrap();
        let found_value = CustomFheAsciiString::from(&entry).decrypt(&client_key);

        assert_eq!(db_size, 1);
        assert_eq!("Hello Value", &found_value);
    }
}
