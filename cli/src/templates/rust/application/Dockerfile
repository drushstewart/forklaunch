FROM rust:1.91-slim-bookworm AS builder

WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev libcurl4-openssl-dev ca-certificates cmake make g++ perl \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release --workspace --bins

FROM debian:bookworm-slim

WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release /app/target/release

ENV HOST=0.0.0.0
ENV PROTOCOL=http
ENV PORT=8000
ENV VERSION=v1
ENV DOCS_PATH=/docs

EXPOSE 8000
