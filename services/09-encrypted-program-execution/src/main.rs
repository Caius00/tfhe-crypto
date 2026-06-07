use crate::cpu::*;
use axum::body::Bytes;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use serde::{Deserialize, Serialize};
use tfhe::{set_server_key, FheBool, FheUint8, ServerKey};

mod cpu;

#[derive(Deserialize, Serialize)]
struct ExecReq {
    server_key: ServerKey,
    cycles: usize,
    a: FheUint8,
    b: FheUint8,
    pc: FheUint8,
    carry: FheBool,
    memory: Vec<FheUint8>,
}

#[derive(Serialize, Deserialize)]
struct ExecResp {
    a: FheUint8,
    b: FheUint8,
    pc: FheUint8,
    carry: FheBool,
    memory: Vec<FheUint8>,
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/execute", post(handle_compute));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handle_compute(body_bytes: Bytes) -> impl IntoResponse {
    let request_data: ExecReq = match bincode::deserialize(&body_bytes) {
        Ok(data) => data,
        Err(_) => return (StatusCode::BAD_REQUEST, "loser").into_response(),
    };

    if request_data.memory.is_empty() {
        return (StatusCode::BAD_REQUEST, "idiot").into_response();
    }

    let mut cpu = make_cpu(request_data.memory.len());

    cpu.a = request_data.a;
    cpu.b = request_data.b;
    cpu.pc = request_data.pc;
    cpu.carry = request_data.carry;
    cpu.memory = request_data.memory;

    let sk = request_data.server_key;
    set_server_key(sk.clone());

    cpu.execute_program(request_data.cycles, &sk);

    match bincode::serialize(&ExecResp {
        a: cpu.a,
        b: cpu.b,
        pc: cpu.pc,
        carry: cpu.carry,
        memory: cpu.memory,
    }) {
        Ok(binary_bytes) => {
            use axum::response::IntoResponse;
            ([("content-type", "application/octet-stream")], binary_bytes).into_response()
        }
        Err(_) => {
            use axum::http::StatusCode;
            (StatusCode::INTERNAL_SERVER_ERROR, "fuck you").into_response()
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::DefaultBodyLimit;
    use axum::http::Request;
    use tfhe::prelude::{FheDecrypt, FheTrivialEncrypt};
    use tfhe::ConfigBuilder;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn fac() {
        let config = ConfigBuilder::default().build();
        let (ck, sk) = tfhe::generate_keys(config);
        set_server_key(sk.clone());

        let mut mem = vec![FheUint8::encrypt_trivial(0u8); 16];

        mem[0] = FheUint8::encrypt_trivial(0x02u8); // LDA immediate
        mem[1] = FheUint8::encrypt_trivial(0x05u8); // data
        mem[2] = FheUint8::encrypt_trivial(0x04u8); // SWP
        mem[3] = FheUint8::encrypt_trivial(0x00u8); // ignored
        mem[4] = FheUint8::encrypt_trivial(0x09u8); // ADD
        mem[5] = FheUint8::encrypt_trivial(0x00u8); // ignored
        mem[6] = FheUint8::encrypt_trivial(0x1Fu8); // DEC
        mem[7] = FheUint8::encrypt_trivial(0x00u8); // ignored
        mem[8] = FheUint8::encrypt_trivial(0x03u8); // LDR
        mem[9] = FheUint8::encrypt_trivial(0x00u8); // ADR
        mem[10] = FheUint8::encrypt_trivial(0x11u8); // MUL
        mem[11] = FheUint8::encrypt_trivial(0x00u8); // ignored
        mem[12] = FheUint8::encrypt_trivial(0x01u8); // LDA
        mem[13] = FheUint8::encrypt_trivial(0x00u8); // Address
        mem[14] = FheUint8::encrypt_trivial(0x08u8); // DJNZ
        mem[15] = FheUint8::encrypt_trivial(0x08u8); // Address

        let pl = ExecReq {
            server_key: sk,
            cycles: 14,
            a: FheUint8::encrypt_trivial(0u8),
            b: FheUint8::encrypt_trivial(0u8),
            pc: FheUint8::encrypt_trivial(0u8),
            carry: FheBool::encrypt_trivial(false),
            memory: mem,
        };
        let serialized = bincode::serialize(&pl).expect("");

        let app = Router::new()
            .route("/compute", post(handle_compute))
            .layer(DefaultBodyLimit::disable());

        let req = Request::builder()
            .method("POST")
            .uri("/compute")
            .header("content-type", "application/octet-stream")
            .body(Body::from(serialized))
            .unwrap();

        let resp = app.oneshot(req).await.expect("");

        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("");

        let rpl: ExecResp = bincode::deserialize(&bytes).expect("");

        let a: u8 = rpl.pc.decrypt(&ck);
        let b: u8 = rpl.a.decrypt(&ck);
        let c: u8 = rpl.b.decrypt(&ck);
        let d: u8 = rpl.memory[0].decrypt(&ck);

        assert_eq!(12u8, a);
        assert_eq!(0u8, b);
        assert_eq!(120u8, c);
        assert_eq!(2u8, d);
    }
}
