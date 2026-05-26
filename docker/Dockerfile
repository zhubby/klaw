FROM debian:bookworm-slim

ARG KLAW_BINARY=target/release/klaw

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

COPY ${KLAW_BINARY} /usr/local/bin/klaw

EXPOSE 18080

VOLUME ["/root/.klaw"]

ENTRYPOINT ["klaw"]
CMD ["gateway"]
