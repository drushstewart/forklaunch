#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PROTOCOL_REDIS_CONTAINER="forklaunch-bullmq-protocol-unit-$$"
trap 'docker rm -f "$PROTOCOL_REDIS_CONTAINER" >/dev/null 2>&1 || true' EXIT
# Upstream script-cache tests issue SCRIPT FLUSH, so use a dedicated server.
docker run --detach --name "$PROTOCOL_REDIS_CONTAINER" --publish 127.0.0.1::6379 --log-driver none redis:7-alpine redis-server --save '' --appendonly no
for _ in $(seq 1 60); do
    if docker exec "$PROTOCOL_REDIS_CONTAINER" redis-cli ping >/dev/null 2>&1; then break; fi
    sleep 1
done
export REDIS_URL="redis://$(docker port "$PROTOCOL_REDIS_CONTAINER" 6379/tcp)/"
CARGO_INCREMENTAL=0 cargo test --manifest-path "$REPO_ROOT/blueprint-rs/crates/vendor/bullmq-official/Cargo.toml" --locked --lib --features forklaunch-outcome-data,forklaunch-job-options --target-dir "${CARGO_TARGET_DIR:-$REPO_ROOT/blueprint-rs/target}" -- --test-threads=1
echo 'PASS pinned BullMQ protocol library tests against isolated Redis'
