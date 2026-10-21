# syntax=docker/dockerfile:1.7
FROM rust:1.98-bookworm AS build
WORKDIR /app
COPY . .
ENV CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    find src vendor/framework -type f -exec touch {} + && \
    cargo build --release --locked --workspace && \
    mkdir -p /out && cp target/release/fieldnotes-notes target/release/fieldnotes-insights /out/

FROM node:24-bookworm-slim AS web-build
WORKDIR /app/web
COPY web/package*.json ./
RUN npm ci --ignore-scripts
COPY web/ ./
COPY --from=build /app/src/client-sdk/typescript/ /app/src/client-sdk/typescript/
RUN npm run build

FROM caddy:2-alpine AS gateway
COPY infra/Caddyfile /etc/caddy/Caddyfile
COPY --from=web-build /app/web/dist /srv

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/fieldnotes-notes /usr/local/bin/
COPY --from=build /out/fieldnotes-insights /usr/local/bin/
USER 65532:65532
CMD ["fieldnotes-notes"]
