#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-worker-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-worker-runtime/run.XXXXXX")"
DB_CONTAINER="forklaunch-rust-worker-test-$$"
OWN_DATABASE=false
ROLE_CREATED=false
export FORKLAUNCH_TEST_ROLE="fl_worker_$(date +%s)_$$"
cleanup() {
    if [ "$ROLE_CREATED" = true ]; then cargo run -p jobs --example seed -- --cleanup >/dev/null 2>&1 || true; fi
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
run_cli init application worker-app --path "$RUN_ROOT/app" --language rust -D 'Worker runtime' -A ForkLaunch -L MIT
run_cli init worker dry-worker --path "$RUN_ROOT/app" --language rust -t database -d postgresql --dryrun
test ! -d "$RUN_ROOT/app/crates/dry-worker"
test ! -d "$RUN_ROOT/app/.forklaunch/blueprint-rs"
run_cli init worker jobs --path "$RUN_ROOT/app" --language rust -t database -d postgresql -D Jobs
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
printf '\npub const USER_OWNED_MARKER: &str = "preserved";\n' >> crates/jobs/src/handler.rs
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> crates/jobs/Cargo.toml
run_cli sync worker jobs --path .
run_cli init worker jobs --path . --language rust -t database -d postgresql -D Jobs
grep -q USER_OWNED_MARKER crates/jobs/src/handler.rs
grep -q 'marker = "preserved"' crates/jobs/Cargo.toml
grep -q 'http::run' crates/jobs/src/main.rs
grep -q 'variant = "worker-postgres"' .forklaunch/manifest.toml
cargo build --workspace
run_cli openapi export --path .
run_cli sdk generate --path . --target all --from contract
env DATABASE_URL="$FORKLAUNCH_TEST_POSTGRES_URL" cargo run -p jobs -- --migrate
mkdir -p crates/jobs/examples
cp "$SCRIPT_DIR/fixtures/rust_worker_seed.rs" crates/jobs/examples/seed.rs
ROLE_CREATED=true
cargo run -p jobs --example seed
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/jobs"))')"
node "$SCRIPT_DIR/rust_worker_runtime.mjs" "$BINARY" stub
cp "$SCRIPT_DIR/fixtures/rust_worker_handler.rs" crates/jobs/src/handler.rs
printf '\npub const USER_OWNED_MARKER: &str = "preserved";\n' >> crates/jobs/src/handler.rs
cargo build -p jobs
node "$SCRIPT_DIR/rust_worker_runtime.mjs" "$BINARY" runtime
cp -R "$RUN_ROOT/relocated-app" "$RUN_ROOT/lifecycle-app"
run_cli change worker --path "$RUN_ROOT/lifecycle-app/crates/jobs" -N email-jobs --confirm
grep -q USER_OWNED_MARKER "$RUN_ROOT/lifecycle-app/crates/email-jobs/src/handler.rs"
grep -q 'http::run' "$RUN_ROOT/lifecycle-app/crates/email-jobs/src/main.rs"
test ! -d "$RUN_ROOT/lifecycle-app/sdks/rust/jobs-sdk"
run_cli sdk generate --path "$RUN_ROOT/lifecycle-app" --target all --from contract --check
run_cli change worker --path "$RUN_ROOT/lifecycle-app/crates/email-jobs" -t redis --confirm
! grep -q 'variant = "worker-postgres"' "$RUN_ROOT/lifecycle-app/.forklaunch/manifest.toml"
run_cli change worker --path "$RUN_ROOT/lifecycle-app/crates/email-jobs" -t database -d postgresql --confirm
grep -q USER_OWNED_MARKER "$RUN_ROOT/lifecycle-app/crates/email-jobs/src/handler.rs"
grep -q 'http::run' "$RUN_ROOT/lifecycle-app/crates/email-jobs/src/main.rs"
grep -q 'marker = "preserved"' "$RUN_ROOT/lifecycle-app/crates/email-jobs/Cargo.toml"
cargo test --manifest-path "$RUN_ROOT/lifecycle-app/Cargo.toml" --workspace
run_cli delete worker email-jobs --path "$RUN_ROOT/lifecycle-app" --continue
test ! -d "$RUN_ROOT/lifecycle-app/crates/email-jobs"
test ! -d "$RUN_ROOT/lifecycle-app/sdks/rust/email-jobs-sdk"
echo 'PASS generated PostgreSQL worker source preservation, relocation, rename, and delete'
