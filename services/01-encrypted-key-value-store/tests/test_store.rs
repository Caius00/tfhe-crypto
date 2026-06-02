use tfhe::{generate_keys, set_server_key, ConfigBuilder};
use tfhe::prelude::{FheDecrypt};
use encrypted_key_value_store::custom_fhe_ascii_string::CustomFheAsciiString;
use encrypted_key_value_store::store::AppState;

#[cfg(test)]
mod test_set{
    use redis::Commands;
    use super::*;

    #[tokio::test]
    async fn single_insert() {
        // CLIENT
        let config = ConfigBuilder::default().build();
        let (client_key, server_key) = generate_keys(config);

        let key = "Hello Key";
        let value = "Hello Value";
        let mut enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);
        // SERVER
        let app_state = AppState::new();
        let mut con = app_state.client.get_connection().unwrap();
        redis::cmd("FLUSHDB").query::<()>(&mut con).unwrap();

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
    /// TODO() how to handle this?
    #[tokio::test]
    async fn different_lengths() {
        // CLIENT
        let config = ConfigBuilder::default().build();
        let (client_key, server_key) = generate_keys(config);

        let key = "Hello Key";
        let value = "Hello Value";
        let mut enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);
        let key_two = "Hello Key Longer";
        let value_two = "Hello Value Longer";
        let mut enc_key_two = CustomFheAsciiString::new(key_two, &client_key);
        let enc_value_two = CustomFheAsciiString::new(value_two, &client_key);
        // SERVER
        let app_state = AppState::new();
        let mut con = app_state.client.get_connection().unwrap();
        redis::cmd("FLUSHDB").query::<()>(&mut con).unwrap();

        let server = tokio::spawn(async move {
            set_server_key(server_key);
            app_state.put(&enc_key, &enc_value).await;
            app_state.put(&enc_key_two, &enc_value_two).await;
            (enc_key, enc_key_two)
        });
        // CLIENT
        (enc_key, enc_key_two) = server.await.unwrap();
        let db_size: usize = redis::cmd("DBSIZE").query(&mut con).unwrap();
        let entry: Vec<u8> = con.get(enc_key.serialize().string).unwrap();
        let entry_two: Vec<u8> = con.get(enc_key_two.serialize().string).unwrap();
        let found_value = CustomFheAsciiString::from(&entry).decrypt(&client_key);
        let found_value_two = CustomFheAsciiString::from(&entry_two).decrypt(&client_key);

        assert_eq!(db_size, 2);
        assert_eq!(value, &found_value);
        assert_eq!(value_two, &found_value_two);
    }
}
