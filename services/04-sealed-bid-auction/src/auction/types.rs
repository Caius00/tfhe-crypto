use axum::http::StatusCode;
use axum::Json;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type ApiError = (StatusCode, String);
pub type ApiResult<T> = Result<Json<T>, ApiError>;

#[allow(dead_code)]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Bid {
    pub bidder_name: String,
    pub encrypted_amount: tfhe::FheUint32,
    pub server_key_bytes: Vec<u8>,
}

#[derive(Deserialize, schemars::JsonSchema)]
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
