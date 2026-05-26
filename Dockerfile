# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        clang \
        cmake \
        git \
        libasound2-dev \
        libatk1.0-dev \
        libcairo2-dev \
        libgdk-pixbuf-2.0-dev \
        libgtk-3-dev \
        libpango1.0-dev \
        libssl-dev \
        libudev-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY . .

RUN rustup target add wasm32-unknown-unknown \
    && WASM_BINDGEN_VERSION="$(awk -F'"' '/^wasm-bindgen = / {print $2; exit}' Cargo.toml)" \
    && cargo install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked \
    && cargo build -p klaw-webui --target wasm32-unknown-unknown --release \
    && mkdir -p klaw-gateway/static/chat/dist \
    && wasm-bindgen target/wasm32-unknown-unknown/release/klaw_webui.wasm \
        --out-dir klaw-gateway/static/chat/dist \
        --target web \
        --no-typescript \
    && cargo build --release -p klaw-cli

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libasound2 \
        libatk1.0-0 \
        libcairo2 \
        libgdk-pixbuf-2.0-0 \
        libgtk-3-0 \
        libpango-1.0-0 \
        libssl3 \
        libx11-6 \
        libxcb1 \
        libxkbcommon0 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/klaw /usr/local/bin/klaw

EXPOSE 18080

VOLUME ["/root/.klaw"]

ENTRYPOINT ["klaw"]
CMD ["gateway"]
