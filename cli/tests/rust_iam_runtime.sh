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
mkdir -p "$SCRIPT_DIR/output/rust-iam-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-iam-runtime/run.XXXXXX")"
DB_CONTAINER="forklaunch-rust-iam-test-$$"
OWN_DATABASE=false
ROLE_CREATED=false
export FORKLAUNCH_TEST_ROLE="fl_gen_$(date +%s)_$$"
cleanup() {
    if [ "$ROLE_CREATED" = true ]; then cargo run -p iam --example seed -- --cleanup >/dev/null 2>&1 || true; fi
    if [ "$OWN_DATABASE" = true ]; then docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT
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
# Argument rejection and dry runs must leave destinations untouched.
run_cli init application dry-iam --path "$RUN_ROOT/dry-app" --language rust --modules iam-base -D 'Dry IAM' -A ForkLaunch -L MIT --dryrun
test ! -e "$RUN_ROOT/dry-app"
if run_cli init application invalid-app --path "$RUN_ROOT/invalid-app" --language rust --modules billing-base -D Invalid -A ForkLaunch -L MIT; then
    echo 'FAIL unsupported Rust modules must be rejected'; exit 1
fi
test ! -e "$RUN_ROOT/invalid-app"
if run_cli init application invalid-db --path "$RUN_ROOT/invalid-db" --language rust --modules iam-base -d sqlite -D Invalid -A ForkLaunch -L MIT; then
    echo 'FAIL IAM must reject unsupported databases'; exit 1
fi
test ! -e "$RUN_ROOT/invalid-db"
run_cli init application bundled-iam --path "$RUN_ROOT/bundled-app" --language rust --modules iam-base -D 'Bundled IAM' -A ForkLaunch -L MIT
test -f "$RUN_ROOT/bundled-app/crates/iam/src/main.rs"
cargo metadata --manifest-path "$RUN_ROOT/bundled-app/Cargo.toml" --no-deps --format-version 1 >/dev/null
run_cli init application iam-app --path "$RUN_ROOT/app" --language rust -D 'IAM runtime' -A ForkLaunch -L MIT
run_cli init module dry-iam --path "$RUN_ROOT/app" --language rust -m iam-base --dryrun
test ! -e "$RUN_ROOT/app/crates/dry-iam"
run_cli init module --path "$RUN_ROOT/app" --language rust -m iam-base -d postgresql
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
printf '\npub const USER_OWNED_MARKER: &str = "preserved";\n' >> crates/iam/src/lib.rs
run_cli sync service iam --path .
grep -q USER_OWNED_MARKER crates/iam/src/lib.rs
grep -q PostgresIamService crates/iam/src/main.rs
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> crates/iam/Cargo.toml
run_cli init module --path . --language rust -m iam-base -d postgresql
grep -q USER_OWNED_MARKER crates/iam/src/lib.rs
grep -q 'marker = "preserved"' crates/iam/Cargo.toml
cargo build --workspace
run_cli openapi export --path .
run_cli sdk generate --path . --target all --from contract
cat > sdks/typescript/src/iam_typecheck.ts <<'TS'
import { createForkLaunchSdk } from './index';
const sdk = createForkLaunchSdk();
async function check() {
  const page = await sdk.iam.users.listUsers({query: {limit: 1}});
  if (page.code === 200) {
    const email: string = page.response.items[0].email;
    const nullable: typeof page.response.next_cursor = null;
    console.log(email, nullable);
  }
  // @ts-expect-error Credentials require a string password.
  await sdk.iam.auth.authenticate({body: {email: 'test@example.com', password: 1}});
}
void check;
TS
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts sdks/typescript/src/iam_typecheck.ts
env DATABASE_URL="$FORKLAUNCH_TEST_POSTGRES_URL" cargo run -p iam -- --migrate
mkdir -p crates/iam/examples
cp "$SCRIPT_DIR/fixtures/rust_iam_seed.rs" crates/iam/examples/seed.rs
ROLE_CREATED=true
cargo run -p iam --example seed
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/iam"))')"
cp "$SCRIPT_DIR/rust_iam_runtime.mjs" sdk_http_test.mjs
cp "$SCRIPT_DIR/rust_mcp_probe.mjs" rust_mcp_probe.mjs
node sdk_http_test.mjs "$BINARY"
# Keep the live fixture intact; validate lifecycle mutations in a separate copy.
cp -R "$RUN_ROOT/relocated-app" "$RUN_ROOT/lifecycle-app"
mkdir -p "$RUN_ROOT/lifecycle-app/crates/occupied"
printf 'user-owned destination\n' > "$RUN_ROOT/lifecycle-app/crates/occupied/marker"
if run_cli change service --path "$RUN_ROOT/lifecycle-app/crates/iam" -N occupied --confirm; then
    echo 'FAIL rename must reject an occupied destination'; exit 1
fi
grep -q 'user-owned destination' "$RUN_ROOT/lifecycle-app/crates/occupied/marker"
test -f "$RUN_ROOT/lifecycle-app/crates/iam/src/main.rs"
run_cli change service --path "$RUN_ROOT/lifecycle-app/crates/iam" -N identity --confirm
test ! -d "$RUN_ROOT/lifecycle-app/sdks/rust/iam-sdk"
test ! -d "$RUN_ROOT/lifecycle-app/.forklaunch/contracts/iam"
test -f "$RUN_ROOT/lifecycle-app/sdks/rust/identity-sdk/Cargo.toml"
grep -q '"name": "identity"' "$RUN_ROOT/lifecycle-app/.forklaunch/contracts/identity/contract.json"
grep -q 'identity:' "$RUN_ROOT/lifecycle-app/sdks/typescript/src/index.ts"
! grep -q 'iam:' "$RUN_ROOT/lifecycle-app/sdks/typescript/src/index.ts"
run_cli sdk generate --path "$RUN_ROOT/lifecycle-app" --target all --from contract --check
cargo check --manifest-path "$RUN_ROOT/lifecycle-app/Cargo.toml" --workspace
run_cli delete service identity --path "$RUN_ROOT/lifecycle-app" --continue
test ! -d "$RUN_ROOT/lifecycle-app/crates/identity"
echo 'PASS Rust IAM module generation, source preservation, rename, and delete'
