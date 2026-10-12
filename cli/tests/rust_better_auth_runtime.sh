#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
mkdir -p "$SCRIPT_DIR/output/rust-better-auth-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-better-auth-runtime/run.XXXXXX")"
export FORKLAUNCH_TEST_ROLE="fl_auth_gen_$(date +%s)_$$"
ROLE_CREATED=false
OWN_DATABASE=false
DB_CONTAINER="forklaunch-rust-better-auth-test-$$"
cleanup() {
    if [ "$ROLE_CREATED" = true ]; then
        cargo run --quiet --manifest-path "$RUN_ROOT/relocated/Cargo.toml" -p iam --example seed -- --cleanup >/dev/null 2>&1 || true
    fi
    if [ "$OWN_DATABASE" = true ]; then docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT
if [ -z "${FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES:-}" ]; then
    export FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES="$RUN_ROOT/client-dependencies"
    npm install --prefix "$FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES" --no-save --package-lock=false --ignore-scripts better-auth@1.7.6
fi
if [ -z "${FORKLAUNCH_TEST_POSTGRES_URL:-}" ]; then
    OWN_DATABASE=true
    docker run --detach --name "$DB_CONTAINER" --publish 127.0.0.1::5432 \
        --tmpfs /var/lib/postgresql/data:rw,size=256m \
        --env POSTGRES_PASSWORD=parity-test-only --env POSTGRES_DB=parity postgres:16-alpine
    for _ in $(seq 1 60); do
        if docker exec "$DB_CONTAINER" pg_isready -U postgres -d parity >/dev/null 2>&1; then break; fi
        sleep 1
    done
    export FORKLAUNCH_TEST_POSTGRES_URL="postgres://postgres:parity-test-only@$(docker port "$DB_CONTAINER" 5432/tcp)/parity"
fi
"$FORKLAUNCH_CLI" init application auth-dry --path "$RUN_ROOT/dry" --language rust --modules iam-better-auth -D Auth -A ForkLaunch -L MIT --dryrun
test ! -e "$RUN_ROOT/dry"
if "$FORKLAUNCH_CLI" init application auth-conflict --path "$RUN_ROOT/conflict" --language rust --modules iam-base iam-better-auth -D Auth -A ForkLaunch -L MIT; then
    echo 'FAIL conflicting IAM presets must be rejected'; exit 1
fi
test ! -e "$RUN_ROOT/conflict"
"$FORKLAUNCH_CLI" init application auth-app --path "$RUN_ROOT/app" --language rust --modules iam-better-auth -D Auth -A ForkLaunch -L MIT
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated"
cd "$RUN_ROOT/relocated"
printf '\npub const USER_OWNED_MARKER: &str = "preserved";\n' >> crates/iam/src/lib.rs
"$FORKLAUNCH_CLI" sync service iam --path .
rg -q USER_OWNED_MARKER crates/iam/src/lib.rs
"$FORKLAUNCH_CLI" init module --path . --language rust -m iam-better-auth -d postgresql
rg -q USER_OWNED_MARKER crates/iam/src/lib.rs
rg -q BETTER_AUTH_SECRET docker-compose.yaml
cargo build -p iam
"$FORKLAUNCH_CLI" openapi export --path .
"$FORKLAUNCH_CLI" sdk generate --path . --target all --from contract
"${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --manifest-path sdks/rust/iam-sdk/Cargo.toml
env DATABASE_URL="$FORKLAUNCH_TEST_POSTGRES_URL" cargo run --quiet -p iam -- --migrate
mkdir -p crates/iam/examples
cp "$SCRIPT_DIR/fixtures/rust_better_auth_seed.rs" crates/iam/examples/seed.rs
ROLE_CREATED=true
cargo run --quiet -p iam --example seed
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/iam"))')"
env FORKLAUNCH_SKIP_FORMAT=1 "$FORKLAUNCH_CLI" init application auth-client --path "$RUN_ROOT/typescript-client" --language typescript --modules-path modules --modules iam-better-auth --database postgresql --validator zod --formatter prettier --linter eslint --http-framework express --runtime node --test-framework vitest -D Auth -A ForkLaunch -L MIT
export FORKLAUNCH_TEST_GENERATED_CLIENT_SDK="$RUN_ROOT/typescript-client/modules/client-sdk/clientSdk.ts"
node "$SCRIPT_DIR/rust_better_auth_runtime.mjs" "$BINARY" "$RUN_ROOT/relocated/sdks/typescript/dist/index.js"
cp -R "$RUN_ROOT/relocated" "$RUN_ROOT/lifecycle"
"$FORKLAUNCH_CLI" change service --path "$RUN_ROOT/lifecycle/crates/iam" -N identity --confirm
test ! -d "$RUN_ROOT/lifecycle/crates/iam"
rg -q USER_OWNED_MARKER "$RUN_ROOT/lifecycle/crates/identity/src/lib.rs"
rg -q 'variant = "iam-better-auth"' "$RUN_ROOT/lifecycle/.forklaunch/manifest.toml"
test -f "$RUN_ROOT/lifecycle/sdks/rust/identity-sdk/Cargo.toml"
"$FORKLAUNCH_CLI" sdk generate --path "$RUN_ROOT/lifecycle" --target all --from contract --check
cargo check --manifest-path "$RUN_ROOT/lifecycle/Cargo.toml" --workspace
"$FORKLAUNCH_CLI" delete service identity --path "$RUN_ROOT/lifecycle" --continue
test ! -d "$RUN_ROOT/lifecycle/crates/identity"
echo 'PASS generated Better Auth preset, relocation, preservation, contracts, both SDKs, HTTP/MCP startup, rename and delete'
