#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-messaging-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-messaging-runtime/run.XXXXXX")"
DB_CONTAINER="forklaunch-rust-sms-test-$$"
OWN_DATABASE=false
ROLE_CREATED=false
export FORKLAUNCH_TEST_ROLE="fl_sms_$(date +%s)_$$"
cleanup() {
    if [ "$ROLE_CREATED" = true ]; then cargo run -p messaging --example seed -- --cleanup >/dev/null 2>&1 || true; fi
    if [ "$OWN_DATABASE" = true ]; then docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT
if [ -z "${FORKLAUNCH_TEST_POSTGRES_URL:-}" ]; then
    OWN_DATABASE=true
    docker run --detach --name "$DB_CONTAINER" --publish 127.0.0.1::5432 --tmpfs /var/lib/postgresql/data:rw,size=256m --env POSTGRES_PASSWORD=parity-test-only --env POSTGRES_DB=parity postgres:16-alpine
    for _ in $(seq 1 60); do
        if docker exec "$DB_CONTAINER" pg_isready -U postgres -d parity >/dev/null 2>&1; then break; fi
        sleep 1
    done
    export FORKLAUNCH_TEST_POSTGRES_URL="postgres://postgres:parity-test-only@$(docker port "$DB_CONTAINER" 5432/tcp)/parity"
fi
run_cli init application dry-app --path "$RUN_ROOT/dry-app" --language rust --modules messaging-twilio -D Dry -A ForkLaunch -L MIT --dryrun
test ! -e "$RUN_ROOT/dry-app"
if run_cli init application conflict-app --path "$RUN_ROOT/conflict-app" --language rust --modules messaging-base messaging-twilio -D Conflict -A ForkLaunch -L MIT; then
    echo 'FAIL conflicting messaging modules must be rejected'; exit 1
fi
test ! -e "$RUN_ROOT/conflict-app"
run_cli init application bundled-app --path "$RUN_ROOT/bundled-app" --language rust --modules messaging-twilio -D Bundled -A ForkLaunch -L MIT
grep -q TwilioSmsService "$RUN_ROOT/bundled-app/crates/messaging/src/main.rs"
run_cli init application sms-app --path "$RUN_ROOT/app" --language mixed --modules messaging-base -D Messaging -A ForkLaunch -L MIT
run_cli init module twilio --path "$RUN_ROOT/app" --language rust -m messaging-twilio
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
printf '\npub const USER_SMS_MARKER: &str = "preserved";\n' >> crates/twilio/src/lib.rs
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> crates/twilio/Cargo.toml
run_cli sync service twilio --path .
run_cli init module twilio --path . --language rust -m messaging-twilio
grep -q USER_SMS_MARKER crates/twilio/src/lib.rs
grep -q TWILIO_STATUS_CALLBACK_URL docker-compose.yaml
if run_cli init service twilio --path . --language rust -d postgresql -D Invalid; then
    echo 'FAIL generic initialization must preserve builtin messaging'; exit 1
fi
cargo build --workspace
OTEL_EXPORTER_OTLP_ENDPOINT=invalid OTEL_EXPORTER_OTLP_PROTOCOL=invalid run_cli openapi export --path .
run_cli sdk generate --path . --target all --from contract
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
# The OpenAPI importer must retain form encoding as well as contract generation.
run_cli sdk generate --path . --target all --from openapi
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
mkdir -p sdks/rust/twilio-sdk/tests
cp "$SCRIPT_DIR/fixtures/rust_sms_sdk_runtime.rs" sdks/rust/twilio-sdk/tests/runtime.rs
cat >> sdks/rust/twilio-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
TOML
python3 - <<'PYTHON'
from pathlib import Path
import json
for name,count in [('messaging',5),('twilio',6)]:
    routes=json.loads((Path('.forklaunch/contracts')/name/'contract.json').read_text())['routes']
    assert len(routes)==count,(name,routes)
PYTHON
env DATABASE_URL="$FORKLAUNCH_TEST_POSTGRES_URL" cargo run -p messaging -- --migrate
mkdir -p crates/messaging/examples
cp "$SCRIPT_DIR/fixtures/rust_sms_seed.rs" crates/messaging/examples/seed.rs
ROLE_CREATED=true
cargo run -p messaging --example seed
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
node "$SCRIPT_DIR/rust_messaging_runtime.mjs" "$TARGET_DIR/debug/messaging" "$TARGET_DIR/debug/twilio" "$PWD/sdks/typescript/dist/index.js"
cargo run -p messaging --example seed -- --inspect
run_cli change service --path crates/twilio -N texting --confirm
grep -q USER_SMS_MARKER crates/texting/src/lib.rs
grep -q 'marker = "preserved"' crates/texting/Cargo.toml
grep -q 'variant = "messaging-twilio"' .forklaunch/manifest.toml
run_cli sync service texting --path .
run_cli sdk generate --path . --target all --from contract --check
cargo check --workspace
run_cli delete service texting --path . --continue
test ! -d crates/texting
test ! -d sdks/rust/texting-sdk
echo 'PASS generated base/Twilio mixed messaging, authenticated runtime, encryption, SDKs, persistence, callbacks, compliance and lifecycle'
