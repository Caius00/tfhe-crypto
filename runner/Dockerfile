FROM ghcr.io/actions/actions-runner:latest

USER root

RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    jq \
    && rm -rf /var/lib/apt/lists/*

USER runner

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    --default-toolchain stable \
    && /home/runner/.cargo/bin/rustup component add rustfmt clippy

RUN ARCH=$(uname -m) && \
    if [ "$ARCH" = "aarch64" ]; then PLATFORM="linux-arm"; else PLATFORM="linux"; fi && \
    curl -LsSf "https://get.nexte.st/latest/${PLATFORM}" | tar zxf - -C /home/runner/.cargo/bin

ENV PATH="/home/runner/.cargo/bin:${PATH}"
