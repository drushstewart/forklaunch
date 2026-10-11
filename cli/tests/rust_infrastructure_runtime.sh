#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
export FORKLAUNCH_CLI
mkdir -p "$SCRIPT_DIR/output/rust-infrastructure-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-infrastructure-runtime/run.XXXXXX")"
node "$SCRIPT_DIR/rust_infrastructure_ownership.mjs" "$RUN_ROOT/app"
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
mkdir -p services/team/storage/examples
cp "$SCRIPT_DIR/fixtures/rust_infrastructure_clients.rs" services/team/storage/examples/infrastructure.rs
cargo build --workspace --examples
cargo build --workspace --bins
env -u REDIS_URL -u S3_URL -u S3_BUCKET -u S3_REGION -u S3_ACCESS_KEY_ID -u S3_SECRET_ACCESS_KEY cargo run -p storage --example infrastructure -- --unconfigured
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
AWS_EC2_METADATA_DISABLED=true \
cargo run -p storage --example infrastructure -- --configured
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
env -u REDIS_URL -u S3_URL -u S3_BUCKET -u S3_REGION -u S3_ACCESS_KEY_ID -u S3_SECRET_ACCESS_KEY node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$TARGET_DIR/debug/storage"
"$FORKLAUNCH_CLI" openapi export --path .
"$FORKLAUNCH_CLI" sdk generate --path . --target all --from contract
"${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
echo 'PASS native Redis/S3 CLI lifecycle, relocated adapters, actual backend round-trips, HTTP/MCP startup and generated SDKs'
