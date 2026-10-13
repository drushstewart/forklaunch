#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
mkdir -p "$SCRIPT_DIR/output/rust-relay-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-relay-runtime/run.XXXXXX")"
export FORKLAUNCH_TEST_ROLE="fl_auth_gen_relay_$(date +%s)_$$"
ROLE_CREATED=false
OWN_DATABASE=false
DB_CONTAINER="forklaunch-rust-relay-test-$$"
cleanup() {
 if [ "$ROLE_CREATED" = true ]; then cargo run --quiet --manifest-path "$RUN_ROOT/relocated/Cargo.toml" -p iam --example seed -- --cleanup; fi
 if [ "$OWN_DATABASE" = true ]; then docker rm -f "$DB_CONTAINER" >/dev/null; fi
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
"$CLI" init application relay-app --path "$RUN_ROOT/app" --language "${FORKLAUNCH_TEST_LANGUAGE:-rust}" --modules iam-better-auth -D Relay -A ForkLaunch -L MIT
"$CLI" init module --path "$RUN_ROOT/app" --language rust -m relay --dryrun
test ! -e "$RUN_ROOT/app/crates/iam/src/relay_hooks.rs"
"$CLI" init module --path "$RUN_ROOT/app" --language rust -m relay
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated"
cd "$RUN_ROOT/relocated"
printf '\n// application-owned relay customization\n' >> crates/iam/src/relay_hooks.rs
cp crates/iam/src/relay_hooks.rs "$RUN_ROOT/hooks.rs"
"$CLI" sync service iam --path .
"$CLI" init module --path . --language rust -m iam-better-auth
cmp crates/iam/src/relay_hooks.rs "$RUN_ROOT/hooks.rs"
if "$CLI" init module --path . --language rust -m relay; then echo 'FAIL duplicate relay install'; exit 1; fi
cargo build -p iam
"$CLI" openapi export --path .
"$CLI" sdk generate --path . --target all --from contract
"${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cargo check --manifest-path sdks/rust/iam-sdk/Cargo.toml
env DATABASE_URL="$FORKLAUNCH_TEST_POSTGRES_URL" cargo run --quiet -p iam -- --migrate
mkdir -p crates/iam/examples
cp "$SCRIPT_DIR/fixtures/rust_better_auth_seed.rs" crates/iam/examples/seed.rs
ROLE_CREATED=true
cargo run --quiet -p iam --example seed
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/iam"))')"
node "$SCRIPT_DIR/rust_relay_runtime.mjs" "$BINARY" "$PWD/sdks/typescript/dist/index.js"
cargo run --quiet -p iam --example seed -- --cleanup
ROLE_CREATED=false
"$CLI" change service --path "$RUN_ROOT/relocated/crates/iam" -N identity --confirm
cmp crates/identity/src/relay_hooks.rs "$RUN_ROOT/hooks.rs"
"$CLI" sdk generate --path . --target all --from contract --check
cargo check --workspace
"$CLI" delete service identity --path . --continue
test ! -d crates/identity
echo 'PASS generated relay relocation, hooks preservation, both SDKs, HTTP/MCP, rename and deletion'
