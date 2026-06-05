use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use tfhe::FheUint32;
use axum::http::StatusCode;
use axum::Json;


pub type ApiError = (StatusCode, String);
pub type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Clone)]
pub struct Bid {
    pub bidder_name: String,
    pub encrypted_amount: FheUint32,
    pub server_key_bytes: Vec<u8>,
}

#[derive(Deserialize, JsonSchema)]
pub struct BidRequest {
    pub bidder_name: String,
    pub encrypted_amount: String, 
    pub server_key: String,       
}

#[derive(Serialize, JsonSchema)]
pub struct AuctionResponse {
    pub status: String,
    pub encrypted_result: String,
}

#[derive(Serialize, JsonSchema)]
pub struct StringResponse {
    pub response: String,
}