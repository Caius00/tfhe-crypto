FROM rust:latest AS chef
RUN cargo install cargo-chef --locked
RUN apt-get update && apt-get install -y cmake clang && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Abhängigkeiten analysieren
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY services/ services/
COPY shared/ shared/
RUN cargo chef prepare --recipe-path recipe.json

# Nur Abhängigkeiten bauen und cachen (inkl. tfhe)
FROM chef AS cacher
# Ziel-CPU: Default ist znver5 (AMD EPYC 9645, Zen 5) für den Produktions-Server.
# Aktiviert AVX-512, VAES, VPCLMULQDQ, GFNI, IFMA, BF16 und VNNI.
# Für lokale Builds oder andere Hardware mit `--build-arg TARGET_CPU=x86-64-v3`
# überschreiben (AVX2-Baseline, läuft auf jeder CPU ab ~Haswell).
#
# Wichtig: CARGO_BUILD_TARGET + CARGO_TARGET_<triple>_RUSTFLAGS trennen
# Host-Code (build.rs, proc-macros, läuft im GitHub-Runner = Intel Xeon)
# von Target-Code (Service-Binary, läuft auf der EPYC). Würde RUSTFLAGS
# global gesetzt, würden auch Build-Skripte mit znver5-Instruktionen
# kompiliert und beim Ausführen im CI-Runner mit SIGILL crashen.
ARG TARGET_CPU=znver5
ENV CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-cpu=${TARGET_CPU}"
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Eigenen Code bauen
FROM chef AS builder
ARG SERVICE_NAME
ARG TARGET_CPU=znver5
ENV CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-cpu=${TARGET_CPU}"
COPY Cargo.toml Cargo.lock ./
COPY services/ services/
COPY shared/ shared/
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo build --release -p $SERVICE_NAME

FROM debian:bookworm-slim
ARG SERVICE_NAME
# Pfad enthält das Target-Triple, weil CARGO_BUILD_TARGET gesetzt ist.
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/$SERVICE_NAME /usr/local/bin/service
CMD ["/usr/local/bin/service"]
