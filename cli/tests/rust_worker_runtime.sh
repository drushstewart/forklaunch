#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
export CARGO_INCREMENTAL=0
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
export FORKLAUNCH_SKIP_FORMAT=1
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-}" != 1 ]; then
    exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "${BASH_SOURCE[0]}" "$@"
fi
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
assert_no_http_sdk() {
    if run_cli sdk generate --path "$1" --target all --from contract --check > "$RUN_ROOT/sdk-check.log" 2>&1; then
        echo 'Consumer-only worker unexpectedly generated an HTTP SDK' >&2
        exit 1
    fi
    grep -q 'No HTTP services or workers found for SDK generation' "$RUN_ROOT/sdk-check.log"
}
mkdir -p "$SCRIPT_DIR/output/rust-worker-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-worker-runtime/run.XXXXXX")"
DB_CONTAINER="forklaunch-rust-worker-test-$$"
OWN_DATABASE=false
OWN_SIGNAL_REDIS=false
SIGNAL_REDIS_CONTAINER="forklaunch-rust-worker-signals-test-$$"
ROLE_CREATED=false
export FORKLAUNCH_TEST_ROLE="fl_worker_$(date +%s)_$$"
export FORKLAUNCH_TEST_TENANT_A="$(node --input-type=module -e 'import {pathToFileURL} from "node:url"; const {secureJobId}=await import(pathToFileURL(process.argv[1])); console.log(secureJobId(process.env.FORKLAUNCH_TEST_ROLE+"-a"));' "$SCRIPT_DIR/fixtures/secure_worker_runtime.mjs")"
export FORKLAUNCH_TEST_TENANT_B="$(node --input-type=module -e 'import {pathToFileURL} from "node:url"; const {secureJobId}=await import(pathToFileURL(process.argv[1])); console.log(secureJobId(process.env.FORKLAUNCH_TEST_ROLE+"-b"));' "$SCRIPT_DIR/fixtures/secure_worker_runtime.mjs")"
cleanup() {
    if [ "$OWN_SIGNAL_REDIS" = true ]; then docker rm -f "$SIGNAL_REDIS_CONTAINER" >/dev/null 2>&1 || true; fi
    if [ "$ROLE_CREATED" = true ]; then cargo run --manifest-path "$RUN_ROOT/relocated-app/Cargo.toml" -p jobs --example seed -- --cleanup >/dev/null 2>&1 || true; fi
    if [ "$OWN_DATABASE" = true ]; then docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true; fi
    if [ "${FORKLAUNCH_KEEP_FIXTURE:-0}" != 1 ]; then rm -rf "$RUN_ROOT"; fi
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
if [ -z "${FORKLAUNCH_TEST_REDIS_URL:-}" ]; then
    OWN_SIGNAL_REDIS=true
    docker run --detach --name "$SIGNAL_REDIS_CONTAINER" --publish 127.0.0.1::6379 \
        --log-driver none redis:7-alpine redis-server --save '' --appendonly no
    for _ in $(seq 1 60); do
        if docker exec "$SIGNAL_REDIS_CONTAINER" redis-cli ping >/dev/null 2>&1; then break; fi
        sleep 1
    done
    export FORKLAUNCH_TEST_REDIS_URL="redis://$(docker port "$SIGNAL_REDIS_CONTAINER" 6379/tcp)/"
fi
run_cli init application worker-app --path "$RUN_ROOT/app" --database postgresql --validator zod --formatter prettier --linter eslint --http-framework express --runtime node --test-framework vitest -D 'Worker runtime' -A ForkLaunch -L MIT
# Exercise the leased encrypted consumer/control runtime explicitly. Default
# accompanying producer generation has separate lifecycle/startup coverage.
run_cli init worker dry-worker --path "$RUN_ROOT/app" --language rust --consumer-only -t database -d postgresql --dryrun
test ! -d "$RUN_ROOT/app/src/dry-worker"
test ! -d "$RUN_ROOT/app/.forklaunch/blueprint-rs"
run_cli init worker jobs --path "$RUN_ROOT/app" --language rust --consumer-only -t database -d postgresql -D Jobs
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
printf '\npub const USER_OWNED_MARKER: &str = "preserved";\n' >> src/jobs/domain/services/handler.rs
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> src/jobs/Cargo.toml
run_cli sync worker jobs --path .
run_cli init worker jobs --path . --language rust --consumer-only -t database -d postgresql -D Jobs
grep -q USER_OWNED_MARKER src/jobs/domain/services/handler.rs
grep -q 'marker = "preserved"' src/jobs/Cargo.toml
grep -q 'signals::run' src/jobs/worker.rs
grep -q 'variant = "worker-postgres"' .forklaunch/manifest.toml
cargo build --workspace
run_cli openapi export --path .
assert_no_http_sdk .
python3 - <<'PYTHON'
from pathlib import Path
import json,tomllib
manifest=tomllib.loads(Path('.forklaunch/manifest.toml').read_text())
worker=next(project for project in manifest['projects'] if project['name']=='jobs')
assert worker['metadata']['workerService'] is False
for filename,key in [('.forklaunch/contracts/jobs/contract.json','routes'),('.forklaunch/openapi/jobs/openapi.json','paths')]:
    artifact=Path(filename)
    if artifact.exists(): assert not json.loads(artifact.read_text())[key], 'Consumer-only worker advertised application HTTP endpoints'
assert not Path('src/jobs/sdk').exists(), 'Consumer-only worker generated an application HTTP SDK'
events=json.loads(Path('src/jobs/events/event-contract.json').read_text())
assert events['transport']=='authenticated-encrypted-event'
assert events['channel']=='work-v1'
assert Path('src/jobs/events/typescript/events.ts').is_file()
assert Path('src/jobs/events/rust/events.rs').is_file()
for source in Path('src/client-sdk').rglob('*.ts'):
    sdk=source.read_text()
    assert 'jobsProcess' not in sdk
    assert 'HealthGet' not in sdk
PYTHON
env DATABASE_URL="$FORKLAUNCH_TEST_POSTGRES_URL" cargo run -p jobs -- --migrate
mkdir -p src/jobs/examples
cp "$SCRIPT_DIR/fixtures/rust_worker_seed.rs" src/jobs/examples/seed.rs
ROLE_CREATED=true
cargo run -p jobs --example seed
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/jobs"))')"
cp "$BINARY" "$RUN_ROOT/jobs-stub"
BINARY="$RUN_ROOT/jobs-stub"
node "$SCRIPT_DIR/rust_worker_transport.mjs" "$BINARY" postgres
node "$SCRIPT_DIR/rust_worker_control_service.mjs" "$BINARY"
node "$SCRIPT_DIR/rust_worker_runtime.mjs" "$BINARY" stub
cp "$SCRIPT_DIR/fixtures/rust_worker_handler.rs" src/jobs/domain/services/handler.rs
printf '\npub const USER_OWNED_MARKER: &str = "preserved";\n' >> src/jobs/domain/services/handler.rs
cargo build -p jobs
BUILD_TARGET="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory))')"
cp "$BUILD_TARGET/debug/jobs" "$RUN_ROOT/jobs-runtime"
BINARY="$RUN_ROOT/jobs-runtime"
node "$SCRIPT_DIR/rust_worker_runtime.mjs" "$BINARY" runtime
node "$SCRIPT_DIR/rust_worker_signals.mjs" "$BINARY"
cp -R "$RUN_ROOT/relocated-app" "$RUN_ROOT/lifecycle-app"
python3 - "$RUN_ROOT/lifecycle-app/src/jobs/domain/schemas/jobs.rs" <<'PYTHON'
from pathlib import Path
import sys
path=Path(sys.argv[1])
source=path.read_text()
assert source.count('    pub message: String,')==1
source=source.replace('    pub message: String,','    pub message: String,\n    pub audit_note: Option<String>,')
source=source.replace('        self.message.zeroize();','        self.message.zeroize();\n        self.audit_note.zeroize();')
path.write_text(source)
PYTHON
cp "$RUN_ROOT/lifecycle-app/src/jobs/domain/schemas/jobs.rs" "$RUN_ROOT/custom-event-schema.rs"
cp "$RUN_ROOT/lifecycle-app/src/jobs/domain/services/handler.rs" "$RUN_ROOT/custom-handler.rs"
run_cli change worker --path "$RUN_ROOT/lifecycle-app/src/jobs" -N email-jobs --confirm
grep -q USER_OWNED_MARKER "$RUN_ROOT/lifecycle-app/src/email-jobs/domain/services/handler.rs"
grep -q 'signals::run' "$RUN_ROOT/lifecycle-app/src/email-jobs/worker.rs"
test ! -d "$RUN_ROOT/lifecycle-app/src/jobs"
test ! -d "$RUN_ROOT/lifecycle-app/src/email-jobs/sdk"
assert_no_http_sdk "$RUN_ROOT/lifecycle-app"
run_cli change worker --path "$RUN_ROOT/lifecycle-app/src/email-jobs" -t redis --confirm
! grep -q 'variant = "worker-postgres"' "$RUN_ROOT/lifecycle-app/.forklaunch/manifest.toml"
cmp "$RUN_ROOT/custom-event-schema.rs" "$RUN_ROOT/lifecycle-app/src/email-jobs/domain/schemas/jobs.rs"
cargo check --manifest-path "$RUN_ROOT/lifecycle-app/Cargo.toml" -p email-jobs --lib --bins
python3 - "$RUN_ROOT/lifecycle-app/src/email-jobs/events" <<'PYTHON'
from pathlib import Path
import json,sys
events=Path(sys.argv[1])
assert 'auditNote' in json.loads((events/'event-contract.json').read_text())['input']['properties']
assert 'auditNote' in (events/'typescript/events.ts').read_text()
assert 'audit_note' in (events/'rust/events.rs').read_text()
PYTHON
run_cli change worker --path "$RUN_ROOT/lifecycle-app/src/email-jobs" -t database -d postgresql --confirm
cmp "$RUN_ROOT/custom-event-schema.rs" "$RUN_ROOT/lifecycle-app/src/email-jobs/domain/schemas/jobs.rs"
cmp "$RUN_ROOT/custom-handler.rs" "$RUN_ROOT/lifecycle-app/src/email-jobs/domain/services/handler.rs"
grep -q USER_OWNED_MARKER "$RUN_ROOT/lifecycle-app/src/email-jobs/domain/services/handler.rs"
grep -q 'signals::run' "$RUN_ROOT/lifecycle-app/src/email-jobs/worker.rs"
grep -q 'marker = "preserved"' "$RUN_ROOT/lifecycle-app/src/email-jobs/Cargo.toml"
cargo test --manifest-path "$RUN_ROOT/lifecycle-app/Cargo.toml" --workspace
run_cli delete worker email-jobs --path "$RUN_ROOT/lifecycle-app" --continue
test ! -d "$RUN_ROOT/lifecycle-app/src/email-jobs"
test ! -d "$RUN_ROOT/lifecycle-app/src/email-jobs/sdk"
echo 'PASS generated PostgreSQL worker source preservation, relocation, rename, and delete'
