use aide::axum::{
    routing::{get_with, post_with},
    ApiRouter,
};
use axum::extract::DefaultBodyLimit;
use tower_http::cors::{Any, CorsLayer};

mod functions;

#[tokio::main]
async fn main() {
    let rayon_threads = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    if let Err(error) = rayon::ThreadPoolBuilder::new()
        .num_threads(rayon_threads)
        .build_global()
    {
        eprintln!("Rayon thread pool already initialized: {error}");
    }
    if let Err((_, error)) = functions::initialize_database().await {
        eprintln!("Genomics database initialization failed: {error}");
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_router = ApiRouter::new()
        .api_route(
            "/fifo",
            get_with(functions::fifo_status_handler, |op| {
                op.description("Show the encrypted genomics processing FIFO")
            }),
        )
        .api_route(
            "/sessions",
            get_with(functions::sessions_handler, |op| {
                op.description("List active encrypted genomics sessions")
            }),
        )
        .api_route(
            "/sessions",
            post_with(functions::create_session_handler, |op| {
                op.description("Create an encrypted genomics session with public and server keys")
            }),
        )
        .api_route(
            "/patterns",
            get_with(functions::patterns_handler, |op| {
                op.description("List cleartext risk patterns stored in the genomics database")
            }),
        )
        .api_route(
            "/patterns",
            post_with(functions::create_pattern_handler, |op| {
                op.description("Create a cleartext risk pattern for database comparisons")
            }),
        )
        .api_route(
            "/sessionsequences",
            get_with(functions::session_sequences_handler, |op| {
                op.description("List sessions with encrypted stored DNA sequences")
            }),
        )
        .api_route(
            "/sessionsequences",
            post_with(functions::store_session_sequence_handler, |op| {
                op.description("Store a public-key encrypted DNA sequence for a genomics session")
            }),
        )
        .api_route(
            "/sessionsequences/hamming",
            post_with(functions::compare_session_sequences_hamming_handler, |op| {
                op.description(
                    "Compare encrypted DNA against stored session sequences with Hamming",
                )
            }),
        )
        .api_route(
            "/sessionsequences/levenshtein",
            post_with(
                functions::compare_session_sequences_levenshtein_handler,
                |op| {
                    op.description(
                        "Compare encrypted DNA against stored session sequences with Levenshtein",
                    )
                },
            ),
        )
        .api_route(
            "/encrypt",
            post_with(functions::encrypt_handler, |op| {
                op.description("Encrypt a server-known DNA sequence with the client's public key")
            }),
        )
        .api_route(
            "/process",
            post_with(functions::process_handler, |op| {
                op.description("Compute Hamming distances over an encrypted DNA sequence")
            }),
        )
        .api_route(
            "/compare-db",
            post_with(functions::compare_database_handler, |op| {
                op.description("Compare encrypted DNA against server-known DNA database entries")
            }),
        )
        .api_route(
            "/process-levenshtein",
            post_with(functions::process_levenshtein_handler, |op| {
                op.description("Compute a Levenshtein distance over encrypted DNA")
            }),
        )
        .api_route(
            "/compare-db-levenshtein",
            post_with(functions::compare_database_levenshtein_handler, |op| {
                op.description(
                    "Compare encrypted DNA against server-known entries using Levenshtein",
                )
            }),
        );

    let app = openapi_docs::attach(
        api_router,
        "Encrypted Genomics",
        "Homomorphic genomics service. The client owns the ClientKey; requests provide a \
         public key for server-side encryption and a server key for computation.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
    .layer(cors);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!(
        "Encrypted Genomics server running on http://{} with {} Rayon threads",
        addr, rayon_threads
    );
    axum::serve(listener, app).await.unwrap();
}

