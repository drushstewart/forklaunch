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
mkdir -p "$SCRIPT_DIR/output/rust-ecommerce-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-ecommerce-runtime/run.XXXXXX")"
DB_CONTAINER="forklaunch-rust-ecommerce-test-$$"
OWN_DATABASE=false
ROLE_CREATED=false
export FORKLAUNCH_TEST_ROLE="fl_ecom_$(date +%s)_$$"
cleanup() {
    if [ "$ROLE_CREATED" = true ]; then cargo run -p ecommerce --example seed -- --cleanup >/dev/null 2>&1 || true; fi
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
run_cli init application dry-app --path "$RUN_ROOT/dry-app" --language rust --modules ecommerce-stripe -D Dry -A ForkLaunch -L MIT --dryrun
test ! -e "$RUN_ROOT/dry-app"
run_cli init application shop-app --path "$RUN_ROOT/app" --language mixed --modules ecommerce-stripe -D Ecommerce -A ForkLaunch -L MIT
run_cli init module spare-shop --path "$RUN_ROOT/app" --language rust -m ecommerce-stripe
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
printf '\npub const USER_SHOP_MARKER: &str = "preserved";\n' >> crates/spare-shop/src/lib.rs
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> crates/spare-shop/Cargo.toml
run_cli sync service spare-shop --path .
run_cli init module spare-shop --path . --language rust -m ecommerce-stripe
grep -q USER_SHOP_MARKER crates/spare-shop/src/lib.rs
grep -q ECOMMERCE_DATABASE_URL docker-compose.yaml
grep -q STRIPE_API_KEY docker-compose.yaml
if run_cli init service spare-shop --path . --language rust -d postgresql -D Invalid; then
    echo 'FAIL generic initialization must preserve builtin ecommerce'; exit 1
fi
cargo build --workspace
OTEL_EXPORTER_OTLP_ENDPOINT=invalid OTEL_EXPORTER_OTLP_PROTOCOL=invalid run_cli openapi export --path .
run_cli sdk generate --path . --target all --from contract
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
run_cli sdk generate --path . --target all --from openapi
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --workspace
mkdir -p sdks/rust/ecommerce-sdk/tests
cp "$SCRIPT_DIR/fixtures/rust_ecommerce_sdk_runtime.rs" sdks/rust/ecommerce-sdk/tests/runtime.rs
cat >> sdks/rust/ecommerce-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
TOML
python3 - <<'PYTHON'
from pathlib import Path
import json
for name in ['ecommerce','spare-shop']:
    routes=json.loads((Path('.forklaunch/contracts')/name/'contract.json').read_text())['routes']
    assert len(routes)==30,(name,routes)
PYTHON
env DATABASE_URL="$FORKLAUNCH_TEST_POSTGRES_URL" cargo run -p ecommerce -- --migrate
mkdir -p crates/ecommerce/examples
cp "$SCRIPT_DIR/fixtures/rust_ecommerce_seed.rs" crates/ecommerce/examples/seed.rs
ROLE_CREATED=true
cargo run -p ecommerce --example seed
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
node "$SCRIPT_DIR/rust_ecommerce_runtime.mjs" "$TARGET_DIR/debug/ecommerce" "$PWD/sdks/typescript/dist/index.js"
cargo run -p ecommerce --example seed -- --inspect
run_cli change service --path crates/spare-shop -N renamed-shop --confirm
grep -q USER_SHOP_MARKER crates/renamed-shop/src/lib.rs
grep -q 'marker = "preserved"' crates/renamed-shop/Cargo.toml
grep -q 'variant = "ecommerce-stripe"' .forklaunch/manifest.toml
run_cli sync service renamed-shop --path .
run_cli sdk generate --path . --target all --from contract --check
cargo check --workspace
run_cli delete service renamed-shop --path . --continue
test ! -d crates/renamed-shop
test ! -d sdks/rust/renamed-shop-sdk
echo 'PASS generated ecommerce mixed application, HTTP/MCP startup, providers, SDKs, inventory worker, persistence and lifecycle'
