use crate::cpu::make_cpu;
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use axum::extract::WebSocketUpgrade;
use axum::routing::get;
use axum::{
    extract::DefaultBodyLimit, http::StatusCode, response::IntoResponse, routing::post, Json,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use bincode::{deserialize, serialize};
use serde::{Deserialize, Serialize};
use serde_json::to_string;
use tfhe::{set_server_key, CompressedServerKey, FheBool, FheUint8};
use tower_http::cors::{Any, CorsLayer};

mod cpu;

#[derive(Deserialize, Serialize)]
struct ExecReq {
    server_key: String,
    cycles: isize,
    a: String,
    b: String,
    pc: String,
    carry: String,
    memory: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct ExecResp {
    a: String,
    b: String,
    pc: String,
    carry: String,
    memory: Vec<String>,
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .route("/execute", post(handle_compute))
        .route("/execute-stream", get(handle_execute_stream))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(DefaultBodyLimit::disable());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

pub fn dd(s: &str) -> Vec<u8> {
    STANDARD.decode(s).expect("")
}

pub(crate) fn ddsk(encoded: &str) -> CompressedServerKey {
    let bytes = STANDARD.decode(encoded).expect("");

    deserialize(&bytes).expect("")
}

pub fn ee<T: serde::Serialize>(value: &T) -> String {
    let raw_bytes = serialize(value).unwrap();
    STANDARD.encode(raw_bytes)
}

async fn handle_compute(Json(req): Json<ExecReq>) -> impl IntoResponse {
    if req.memory.is_empty() {
        return (StatusCode::BAD_REQUEST, "idiot").into_response();
    }

    let sk = ddsk(&req.server_key).decompress();
    set_server_key(sk.clone());

    let fhe_a: FheUint8 = deserialize(&dd(&req.a)).unwrap();
    let fhe_b: FheUint8 = deserialize(&dd(&req.b)).unwrap();
    let fhe_pc: FheUint8 = deserialize(&dd(&req.pc)).unwrap();
    let fhe_carry: FheBool = deserialize(&dd(&req.carry)).unwrap();

    let mut fhe_memory: Vec<FheUint8> = Vec::with_capacity(req.memory.len());
    for cell_b64 in &req.memory {
        let cell_bytes = dd(cell_b64);
        fhe_memory.push(deserialize(&cell_bytes).unwrap());
    }

    let mut cpu = make_cpu(fhe_memory.len());
    cpu.a = fhe_a;
    cpu.b = fhe_b;
    cpu.pc = fhe_pc;
    cpu.carry = fhe_carry;
    cpu.memory = fhe_memory;

    for _ in 0..req.cycles {
        cpu.execute_program(&sk);
    }

    let resp = ExecResp {
        a: ee(&cpu.a),
        b: ee(&cpu.b),
        pc: ee(&cpu.pc),
        carry: ee(&cpu.carry),
        memory: cpu.memory.iter().map(ee).collect(),
    };

    Json(resp).into_response()
}

pub async fn handle_execute_stream(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.max_message_size(usize::MAX)
        .max_frame_size(usize::MAX)
        .on_failed_upgrade(|error| println!("{}", error))
        .on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    let raw_message = match socket.recv().await {
        Some(Ok(msg)) => msg,
        Some(Err(_)) => {
            return;
        }
        None => {
            return;
        }
    };

    let text_payload = match raw_message {
        Message::Text(text) => text,
        _ => return,
    };

    let req: ExecReq = match serde_json::from_str(&text_payload) {
        Ok(r) => r,
        Err(_) => {
            let _ = socket.send(Message::Text(Utf8Bytes::from("idiot"))).await;
            return;
        }
    };

    let sk = ddsk(&req.server_key).decompress();

    set_server_key(sk.clone());

    let fhe_a: FheUint8 = deserialize(&dd(&req.a)).unwrap();
    let fhe_b: FheUint8 = deserialize(&dd(&req.b)).unwrap();
    let fhe_pc: FheUint8 = deserialize(&dd(&req.pc)).unwrap();
    let fhe_carry: FheBool = deserialize(&dd(&req.carry)).unwrap();

    let mut fhe_memory: Vec<FheUint8> = Vec::with_capacity(req.memory.len());
    for cell_b64 in &req.memory {
        let cell_bytes = dd(cell_b64);
        fhe_memory.push(deserialize(&cell_bytes).unwrap());
    }

    let mut cpu = make_cpu(fhe_memory.len());
    cpu.a = fhe_a;
    cpu.b = fhe_b;
    cpu.pc = fhe_pc;
    cpu.carry = fhe_carry;
    cpu.memory = fhe_memory;

    set_server_key(sk.clone());

    if req.cycles <= 0 {
        return;
    }

    for _ in 0..req.cycles {
        cpu.execute_program(&sk);

        let state = ExecResp {
            a: ee(&cpu.a),
            b: ee(&cpu.b),
            pc: ee(&cpu.pc),
            carry: ee(&cpu.carry),
            memory: cpu.memory.iter().map(ee).collect(),
        };

        let msg = Utf8Bytes::from(to_string(&state).unwrap());

        if socket.send(Message::Text(msg)).await.is_err() {
            return;
        }
    }
}
