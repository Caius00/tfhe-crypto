FROM rust:latest AS chef
RUN cargo install cargo-chef --locked
RUN apt-get update && apt-get install -y cmake clang && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Abhängigkeiten analysieren
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY services/ services/
RUN cargo chef prepare --recipe-path recipe.json

# Nur Abhängigkeiten bauen und cachen (inkl. tfhe)
FROM chef AS cacher
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Eigenen Code bauen
FROM chef AS builder
ARG SERVICE_NAME
COPY Cargo.toml Cargo.lock ./
COPY services/ services/
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo build --release -p $SERVICE_NAME

FROM debian:bookworm-slim
ARG SERVICE_NAME
COPY --from=builder /app/target/release/$SERVICE_NAME /usr/local/bin/service
CMD ["/usr/local/bin/service"]
