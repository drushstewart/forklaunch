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
mkdir -p "$SCRIPT_DIR/output/rust-billing-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-billing-runtime/run.XXXXXX")"
DB_CONTAINER="forklaunch-rust-billing-test-$$"
OWN_DATABASE=false
ROLE_CREATED=false
export FORKLAUNCH_TEST_ROLE="fl_billing_$(date +%s)_$$"
cleanup() {
    if [ "$ROLE_CREATED" = true ]; then cargo run -p billing --example seed -- --cleanup >/dev/null 2>&1 || true; fi
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
run_cli init application dry-app --path "$RUN_ROOT/dry-app" --language rust --modules billing-stripe -D Dry -A ForkLaunch -L MIT --dryrun
test ! -e "$RUN_ROOT/dry-app"
if run_cli init application conflict-app --path "$RUN_ROOT/conflict-app" --language rust --modules billing-base billing-stripe -D Conflict -A ForkLaunch -L MIT; then
    echo 'FAIL conflicting billing modules must be rejected'; exit 1
fi
test ! -e "$RUN_ROOT/conflict-app"
run_cli init application bundled-app --path "$RUN_ROOT/bundled-app" --language rust --modules billing-stripe -D Bundled -A ForkLaunch -L MIT
grep -q StripeBillingClient "$RUN_ROOT/bundled-app/crates/billing/src/main.rs"
run_cli init application billing-app --path "$RUN_ROOT/app" --language mixed --modules billing-base -D Billing -A ForkLaunch -L MIT
run_cli init module stripe --path "$RUN_ROOT/app" --language rust -m billing-stripe
for feature in email sms whatsapp voice payments; do
    run_cli infra add stripe "$feature" --path "$RUN_ROOT/app"
done
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
printf '\npub const USER_BILLING_MARKER: &str = "preserved";\n' >> crates/stripe/src/lib.rs
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> crates/stripe/Cargo.toml
run_cli sync service stripe --path .
run_cli init module stripe --path . --language rust -m billing-stripe
grep -q USER_BILLING_MARKER crates/stripe/src/lib.rs
grep -q STRIPE_SECRET_KEY docker-compose.yaml
if run_cli init service stripe --path . --language rust -d postgresql -D Invalid; then
    echo 'FAIL generic initialization must preserve builtin billing'; exit 1
fi
cargo build --workspace
OTEL_EXPORTER_OTLP_ENDPOINT=invalid OTEL_EXPORTER_OTLP_PROTOCOL=invalid run_cli openapi export --path .
run_cli sdk generate --path . --target all --from contract
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
# Exercise both contract and OpenAPI imports for configured billing schemas.
run_cli sdk generate --path . --target all --from openapi
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
for name in billing stripe; do
    mkdir -p "sdks/rust/$name-sdk/tests"
    python3 - "$SCRIPT_DIR/fixtures/rust_billing_sdk_runtime.rs" "$name" <<'PYTHON_SDK'
from pathlib import Path
import sys,re
source,name=sys.argv[1:]
text=Path(source).read_text()
if name=='stripe':text=re.sub(r'\bBilling','Stripe',text.replace('billing_sdk','stripe_sdk'))
(Path('sdks/rust')/(name+'-sdk')/'tests/runtime.rs').write_text(text)
PYTHON_SDK
    cat >> "sdks/rust/$name-sdk/Cargo.toml" <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
TOML
done
python3 - <<'PYTHON'
from pathlib import Path
import json
for name in ['billing','stripe']:
    routes=json.loads((Path('.forklaunch/contracts')/name/'contract.json').read_text())['routes']
    assert len(routes)==(35 if name=="stripe" else 33),(name,routes)
PYTHON
env DATABASE_URL="$FORKLAUNCH_TEST_POSTGRES_URL" cargo run -p billing -- --migrate
mkdir -p crates/billing/examples
cp "$SCRIPT_DIR/fixtures/rust_billing_seed.rs" crates/billing/examples/seed.rs
ROLE_CREATED=true
cargo run -p billing --example seed
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
node "$SCRIPT_DIR/rust_billing_runtime.mjs" "$TARGET_DIR/debug/billing" "$TARGET_DIR/debug/stripe" "$PWD/sdks/typescript/dist/index.js"
cargo run -p billing --example seed -- --inspect
run_cli change service --path crates/stripe -N payments --confirm
grep -q USER_BILLING_MARKER crates/payments/src/lib.rs
grep -q 'marker = "preserved"' crates/payments/Cargo.toml
grep -q 'variant = "billing-stripe"' .forklaunch/manifest.toml
run_cli sync service payments --path .
run_cli sdk generate --path . --target all --from contract --check
cargo check --workspace
run_cli delete service payments --path . --continue
test ! -d crates/payments
test ! -d sdks/rust/payments-sdk
echo 'PASS generated core billing/Stripe mixed modules, HTTP/MCP startup, SDKs, encrypted persistence and CLI lifecycle'
