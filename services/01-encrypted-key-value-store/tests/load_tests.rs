// WIP
//
// #[cfg(test)]
// mod idk {
//     use std::time::Duration;
//     use reqwest::{Client, Error};
//     use tfhe::{set_server_key, ClientKey, CompactCiphertextList, CompactCiphertextListBuilder, CompactPublicKey, CompressedServerKey};
//     use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
//     use tokio::time::sleep;
//     use encrypted_key_value_store::custom_fhe_ascii_string::CustomFheAsciiString;
//     use encrypted_key_value_store::models::{CreateSessionRequest, MessageResponse, PutRequest};
//
//     fn route_compress(
//         s: &str,
//         compact_pk: &CompactPublicKey,
//     ) -> CompactCiphertextList {
//         let mut builder = CompactCiphertextListBuilder::new(compact_pk);
//
//         for byte in s.bytes() {
//             builder.push(byte);
//         }
//
//         builder.build()
//     }
//
//     async fn run_client(key: &str, value: &str) {
//         let parameters = MetaParametersFinder::new(
//             Constraint::LessThanOrEqual(Log2PFail(-128.0)),
//             Backend::Cpu,
//         )
//             .with_compression(true)
//             .find()
//             .expect("Could not find suitable parameters");
//
//         let client_key = ClientKey::generate(parameters);
//         let compressed_server_key = CompressedServerKey::new(&client_key);
//         set_server_key(compressed_server_key.decompress());
//
//         let enc_key = CustomFheAsciiString::new(key, &client_key);
//         let enc_value = CustomFheAsciiString::new(value, &client_key);
//
//         test_all(
//             compressed_server_key,
//             enc_key,
//             enc_value,
//             value,
//             &client_key,
//         )
//             .await;
//     }
//
//     async fn test_all(
//         compressed_server_key: CompressedServerKey,
//         enc_key: CustomFheAsciiString,
//         enc_value: CustomFheAsciiString,
//         value: &str,
//         client_key: &ClientKey,
//     ) {
//         // Setup
//         let session_id = create_session_route(&compressed_server_key).await;
//
//         // Check initial exists
//         let initial_exists = exists_req(&enc_key, &session_id, client_key).await;
//         assert!(!initial_exists);
//
//         // Put
//         put_req(&enc_key, &enc_value, &session_id).await;
//
//         // Check Successful Put
//         let put_exists = exists_req(&enc_key, &session_id, client_key).await;
//         assert!(put_exists);
//         println!("Passed Put test.");
//
//         // Get
//         let response_value = get_req(&enc_value, &session_id, client_key).await;
//         assert_eq!(value, response_value);
//         println!("Passed Get test.");
//
//         // Delete
//         delete_req(&enc_key, &session_id).await;
//
//         // Check Successful Delete
//         let delete_exists = exists_req(&enc_key, &session_id, client_key).await;
//         assert!(!delete_exists);
//         println!("Passed Delete test.");
//     }
//
//     async fn create_session_route(
//         client: &Client,
//         compressed_server_key: &CompressedServerKey,
//     ) -> Result<String, Error> {
//         let server_key = bincode::serialize(compressed_server_key)?;
//
//         let body: MessageResponse = client
//             .post(format!("{BASE_ADDRESS}/session"))
//             .json(&CreateSessionRequest { server_key })
//             .send()
//             .await?
//             .error_for_status()?
//             .json()
//             .await?;
//
//         Ok(body.message)
//     }
//
//     async fn put_entry_route(
//         client: &Client,
//         key: Vec<u8>,
//         value: Vec<u8>,
//         session_id: String,
//     ) -> Result<(), Error> {
//         client
//             .post(format!("{BASE_ADDRESS}/entry"))
//             .json(&PutRequest { key, value, session_id })
//             .send()
//             .await?
//             .error_for_status()?;
//
//         Ok(())
//     }
//
//     async fn clear_request() -> Result<(), Error> {
//         let client = Client::new();
//         client.delete(format!("{}/clear", BASE_ADDRESS))
//             .send()
//             .await?;
//
//         println!("Clearing DB");
//
//         Ok(())
//     }
//
//     async fn roundtrip(
//         client: &Client,
//         compressed_server_key: CompressedServerKey,
//         enc_key: CustomFheAsciiString,
//         enc_value: CustomFheAsciiString,
//         value: &str,
//         client_key: &ClientKey,
//     ) -> Result<(), Error>{
//         let idk_a = enc_key.compress().string;
//         let idk_b = enc_value.compress().string;
//
//
//         clear_request().await?;
//         let session_id = create_session_route(&client, &compressed_server_key).await?;
//         put_entry_route(&client, &enc_key, &enc_value, session_id).await?;
//
//
//         Ok(())
//     }
//
//     const BASE_ADDRESS: &str = "http://159.195.145.100/kv"; // TODO() choose correct endpoint
//     #[tokio::test]
//     async fn main() -> Result<(), Box<dyn std::error::Error>> {
//         clear_request().await?;
//
//         sleep(Duration::from_millis(100)).await;
//
//         let c1 = std::thread::spawn(move || {
//             let rt = tokio::runtime::Builder::new_current_thread()
//                 .enable_all()
//                 .build()
//                 .unwrap();
//             rt.block_on(run_client("Hello Key A", "Hello Value A"))
//         });
//
//         let c2 = std::thread::spawn(move || {
//             let rt = tokio::runtime::Builder::new_current_thread()
//                 .enable_all()
//                 .build()
//                 .unwrap();
//             rt.block_on(run_client("Hello Key B", "Hello Value B"))
//         });
//
//         c1.join().unwrap();
//         c2.join().unwrap();
//
//         Ok(())
//     }
// }
