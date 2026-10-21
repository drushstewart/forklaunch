#!/usr/bin/env bash
set -euo pipefail
export FORKLAUNCH_FRAMEWORK_PATH="${FORKLAUNCH_FRAMEWORK_PATH:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/framework-rs}"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
export FORKLAUNCH_CLI
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
export CARGO_INCREMENTAL=0
export FORKLAUNCH_SKIP_FORMAT=1
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-0}" != 1 ]; then
    exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "$SCRIPT_DIR/rust_infrastructure_runtime.sh"
fi
mkdir -p "$SCRIPT_DIR/output/rust-infrastructure-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-infrastructure-runtime/run.XXXXXX")"
REDIS_CONTAINER=""
S3_CONTAINER=""
cleanup() {
    fixture_status=$?
    for fixture_container in "$REDIS_CONTAINER" "$S3_CONTAINER"; do
        if [ -n "$fixture_container" ]; then docker rm -f "$fixture_container" >/dev/null; fi
    done
    if [ "${FORKLAUNCH_KEEP_FIXTURE:-0}" != 1 ]; then rm -rf "$RUN_ROOT"; fi
    exit "$fixture_status"
}
trap cleanup EXIT
node "$SCRIPT_DIR/rust_infrastructure_ownership.mjs" "$RUN_ROOT/app"
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
mkdir -p services/team/storage/examples
cp "$SCRIPT_DIR/fixtures/rust_infrastructure_clients.rs" services/team/storage/examples/infrastructure.rs
cp "$SCRIPT_DIR/fixtures/rust_record_codec_interop.rs" services/team/storage/examples/record_interop.rs
cargo build --workspace --examples
cargo build --workspace --bins
env -u REDIS_URL -u S3_URL -u S3_BUCKET -u S3_REGION -u S3_ACCESS_KEY_ID -u S3_SECRET_ACCESS_KEY cargo run -p storage --example infrastructure -- --unconfigured
# Each run owns its stores. Random loopback ports avoid touching developer stacks.
if [ -z "${FORKLAUNCH_TEST_REDIS_URL:-}" ]; then
    REDIS_CONTAINER="$(docker run -d --rm -p 127.0.0.1::6379 redis:7-alpine)"
    REDIS_PORT="$(docker inspect "$REDIS_CONTAINER" --format '{{(index (index .NetworkSettings.Ports "6379/tcp") 0).HostPort}}')"
    export FORKLAUNCH_TEST_REDIS_URL="redis://127.0.0.1:$REDIS_PORT/12"
    for attempt in $(seq 1 60); do
        if docker exec "$REDIS_CONTAINER" redis-cli ping | grep -q PONG; then break; fi
        sleep 1
    done
    docker exec "$REDIS_CONTAINER" redis-cli ping | grep -q PONG
fi
if [ -z "${FORKLAUNCH_TEST_S3_ENDPOINT:-}" ]; then
    export FORKLAUNCH_TEST_S3_ACCESS_KEY=parity-test-access
    export FORKLAUNCH_TEST_S3_SECRET_KEY=parity-test-secret-only
    S3_CONTAINER="$(docker run -d --rm -p 127.0.0.1::9000 -e MINIO_ROOT_USER="$FORKLAUNCH_TEST_S3_ACCESS_KEY" -e MINIO_ROOT_PASSWORD="$FORKLAUNCH_TEST_S3_SECRET_KEY" cgr.dev/chainguard/minio:latest server /data)"
    S3_PORT="$(docker inspect "$S3_CONTAINER" --format '{{(index (index .NetworkSettings.Ports "9000/tcp") 0).HostPort}}')"
    export FORKLAUNCH_TEST_S3_ENDPOINT="http://127.0.0.1:$S3_PORT"
    python3 - <<'PY'
import os, time, urllib.request
for attempt in range(60):
    try:
        with urllib.request.urlopen(os.environ['FORKLAUNCH_TEST_S3_ENDPOINT']+'/minio/health/live', timeout=1) as response:
            if response.status == 200: break
    except OSError:
        pass
    time.sleep(1)
else:
    raise RuntimeError('Owned S3 fixture did not become healthy')
PY
fi
REDIS_URL="${FORKLAUNCH_TEST_REDIS_URL:-redis://127.0.0.1:32770/12}" \
S3_URL="${FORKLAUNCH_TEST_S3_ENDPOINT:-http://127.0.0.1:32776}" \
S3_REGION=us-east-1 S3_BUCKET=forklaunch-native-infra-fixture \
S3_ACCESS_KEY_ID="${FORKLAUNCH_TEST_S3_ACCESS_KEY:-parity-test-access}" \
S3_SECRET_ACCESS_KEY="${FORKLAUNCH_TEST_S3_SECRET_KEY:-parity-test-secret-only}" \
cargo run -p storage --example infrastructure
# Deployed services can use the default AWS chain without static S3 credential settings.
env -u S3_ACCESS_KEY_ID -u S3_SECRET_ACCESS_KEY -u S3_REGION \
REDIS_URL="${FORKLAUNCH_TEST_REDIS_URL:-redis://127.0.0.1:32770/12}" \
S3_URL="${FORKLAUNCH_TEST_S3_ENDPOINT:-http://127.0.0.1:32776}" \
AWS_REGION=us-east-1 S3_BUCKET=forklaunch-native-infra-fixture S3_PREFIX=/generated/tenant/ \
AWS_ACCESS_KEY_ID="${FORKLAUNCH_TEST_S3_ACCESS_KEY:-parity-test-access}" \
AWS_SECRET_ACCESS_KEY="${FORKLAUNCH_TEST_S3_SECRET_KEY:-parity-test-secret-only}" \
AWS_EC2_METADATA_DISABLED=true ENCRYPTION_KEY=fixture-only-native-infrastructure-encryption \
cargo run -p storage --example infrastructure -- --configured
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
# Build the actual generated framework SDK package in an isolated installation.
# This does not purge or depend on the developer's monorepo node_modules.
mkdir -p "$RUN_ROOT/typescript-runtime" && (cd "$REPO_ROOT/framework/universal-sdk" && cp -R index.ts src package.json tsconfig.json tsconfig.build.json "$RUN_ROOT/typescript-runtime/")
npm install --prefix "$RUN_ROOT/typescript-runtime" --ignore-scripts --package-lock=false --save-dev tsx@4.23.15
npm --prefix "$RUN_ROOT/typescript-runtime" run build
"${TSX_BIN:-$RUN_ROOT/typescript-runtime/node_modules/.bin/tsx}" "$SCRIPT_DIR/rust_record_codec_interop.ts" "$TARGET_DIR/debug/examples/record_interop"
env -u REDIS_URL -u S3_URL -u S3_BUCKET -u S3_REGION -u S3_ACCESS_KEY_ID -u S3_SECRET_ACCESS_KEY node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$TARGET_DIR/debug/storage"
"$FORKLAUNCH_CLI" openapi export --path .
"$FORKLAUNCH_CLI" sdk generate --path . --target all --from contract
"$FORKLAUNCH_CLI" sdk generate --path . --target all --from contract --check
mkdir -p services/team/storage/sdk/typescript/node_modules/@forklaunch
ln -s "$RUN_ROOT/typescript-runtime" services/team/storage/sdk/typescript/node_modules/@forklaunch/universal-sdk
"${TSC_BIN:-$RUN_ROOT/typescript-runtime/node_modules/.bin/tsc}" --project services/team/storage/sdk/typescript/tsconfig.json
cargo check --workspace
echo 'PASS native Redis/S3 CLI lifecycle, relocated adapters, actual backend round-trips, HTTP/MCP startup and generated SDKs'
