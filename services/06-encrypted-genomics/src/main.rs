use aide::axum::{routing::post_with, ApiRouter};
use axum::extract::DefaultBodyLimit;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

mod functions;

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let shared_state = Arc::new(functions::AppState::new());

    let api_router = ApiRouter::new()
        .api_route(
            "/encrypt",
            post_with(functions::encrypt_handler, |op| {
                op.description("Encrypt dna")
            }),
        )
        .api_route(
            "/process",
            post_with(functions::process_handler, |op| {
                op.description("Check homomorph risk pattern hamming")
            }),
        )
        .api_route(
            "/decrypt",
            post_with(functions::decrypt_handler, |op| {
                op.description("Decrypt results homomorph risk-pattern hamming")
            }),
        )
        .api_route(
            "/compare-db",
            post_with(functions::compare_database_handler, |op| {
                op.description("Compare encrypted DNA against encrypted DNA database")
            }),
        )
        .api_route(
            "/process-levenshtein",
            post_with(functions::process_levenshtein_handler, |op| {
                op.description("Check homomorph risk pattern levenshtein")
            }),
        )
        .api_route(
            "/compare-db-levenshtein",
            post_with(functions::compare_database_levenshtein_handler, |op| {
                op.description("Compare encrypted DNA using levenshtein")
            }),
        )
        .with_state(shared_state);

    let app = openapi_docs::attach(api_router, "tammoloco", "0.1", "")
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
        .layer(cors);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

/*

curl
# encrypt
enc=$(curl -s -X POST http://localhost:8080/encrypt \
  -H "Content-Type: application/json" \
  -d '{"sequence":"ATCGATCG"}')

echo "$enc"

# process
proc=$(echo "$enc" | \
jq --arg risk "012" '
{
    encrypted_sequence: .encrypted_data,
    risk_pattern: $risk
}' | \
curl -s -X POST http://localhost:8080/process \
    -H "Content-Type: application/json" \
    --data @-)

echo "$proc"

# decrypt
dec=$(echo "$proc" | \
jq '
{
    encrypted_data: .encrypted_distances
}' | \
curl -s -X POST http://localhost:8080/decrypt \
    -H "Content-Type: application/json" \
    --data @-)

echo "$dec" | jq '.plain_data'



win
# use format list to see sequence original length ($enc | format-list), window length ($proc | format-list)
# encrypt testsequence in body
    $enc = Invoke-RestMethod `
    -Uri "http://localhost:8080/encrypt" `
    -Method Post `
    -ContentType "application/json" `
    -Body '{"sequence":"ATCGATCGAAAA"}'
    $enc


# check risk pattern with hamming distance , risk pattern in body
    $proc = Invoke-RestMethod `
    -Uri "http://localhost:8080/process" `
    -Method Post `
    -ContentType "application/json" `
    -Body (@{
        encrypted_sequence = $enc.encrypted_data
        risk_pattern = "012"
    } | ConvertTo-Json)

    $proc

# decrypt risk marker hamming compare
    $dec = Invoke-RestMethod `
    -Uri "http://localhost:8080/decrypt" `
    -Method Post `
    -ContentType "application/json" `
    -Body (@{
        encrypted_data = $proc.encrypted_distances
    } | ConvertTo-Json)

    $dec.plain_data

# check dna against every other stored dna in db (currently just function stored values)
    $cmp = Invoke-RestMethod `
        -Uri "http://localhost:8080/compare-db" `
        -Method Post `
        -ContentType "application/json" `
        -Body (@{
            encrypted_sequence = $enc.encrypted_data
        } | ConvertTo-Json)

    $cmp

# decrypt dna-dna from db compares, currently has 3 values callable by [value], so [0], [1], [2]

   $allResults = @{}

    for($i = 0; $i -lt $cmp.encrypted_results.Count; $i++) {

        $dec = Invoke-RestMethod `
            -Uri "http://localhost:8080/decrypt" `
            -Method Post `
            -ContentType "application/json" `
            -Body (@{
                encrypted_data = $cmp.encrypted_results[$i]
            } | ConvertTo-Json)

        $allResults["db_sequence_$i"] = $dec.plain_data
    }

    foreach($key in $allResults.Keys) {
        Write-Host ""
        Write-Host "$key"
        Write-Host ($allResults[$key] -join ", ")
    }


    # levenshtein single
    $procLev = Invoke-RestMethod `
        -Uri "http://localhost:8080/process-levenshtein" `
        -Method Post `
        -ContentType "application/json" `
        -Body (@{
            encrypted_sequence = $enc.encrypted_data
            risk_pattern = "012"
        } | ConvertTo-Json)

    $procLev

    # decode levenshtein single
    $decLev = Invoke-RestMethod `
        -Uri "http://localhost:8080/decrypt-levenshtein" `
        -Method Post `
        -ContentType "application/json" `
        -Body (@{
            encrypted_data = $procLev.encrypted_distances
        } | ConvertTo-Json)

    $decLev.plain_data

    # db levenshtein
        $cmpLev = Invoke-RestMethod `
        -Uri "http://localhost:8080/compare-db-levenshtein" `
        -Method Post `
        -ContentType "application/json" `
        -Body (@{
            encrypted_sequence = $enc.encrypted_data
        } | ConvertTo-Json)

    $cmpLev

    # result db levenshtein
    $allResults = @{}

    for($i = 0; $i -lt $cmpLev.encrypted_results.Count; $i++) {

        $dec = Invoke-RestMethod `
            -Uri "http://localhost:8080/decrypt-levenshtein" `
            -Method Post `
            -ContentType "application/json" `
            -Body (@{
                encrypted_data = $cmpLev.encrypted_results[$i]
            } | ConvertTo-Json)

        $allResults["db_sequence_$i"] = $dec.plain_data
    }

    foreach($key in $allResults.Keys) {

        Write-Host ""
        Write-Host $key
        Write-Host ($allResults[$key] -join ", ")
    }

*/
