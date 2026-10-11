#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
REDIS_CONTAINER="forklaunch-rust-bullmq-test-$$"
OWN_REDIS=false
cleanup() { if [ "$OWN_REDIS" = true ]; then docker rm -f "$REDIS_CONTAINER" >/dev/null 2>&1 || true; fi; }
trap cleanup EXIT
if [ -z "${FORKLAUNCH_TEST_REDIS_URL:-}" ]; then
    OWN_REDIS=true
    docker run --detach --name "$REDIS_CONTAINER" --publish 127.0.0.1::6379 --log-driver none redis:7-alpine redis-server --save '' --appendonly no
    for _ in $(seq 1 60); do
        if docker exec "$REDIS_CONTAINER" redis-cli ping >/dev/null 2>&1; then break; fi
        sleep 1
    done
    export FORKLAUNCH_TEST_REDIS_URL="redis://$(docker port "$REDIS_CONTAINER" 6379/tcp)/"
fi
mkdir -p "$SCRIPT_DIR/output/rust-bullmq-worker-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-bullmq-worker-runtime/run.XXXXXX")"
"$FORKLAUNCH_CLI" init application worker-app --path "$RUN_ROOT/app" --language rust -D 'BullMQ runtime' -A ForkLaunch -L MIT
"$FORKLAUNCH_CLI" init worker dry-worker --path "$RUN_ROOT/app" --language rust -t bullmq --dryrun
test ! -d "$RUN_ROOT/app/crates/dry-worker"
test ! -d "$RUN_ROOT/app/.forklaunch/blueprint-rs"
"$FORKLAUNCH_CLI" init worker jobs --path "$RUN_ROOT/app" --language rust -t bullmq -D Jobs
"$FORKLAUNCH_CLI" init service api --path "$RUN_ROOT/app" --language rust -D API
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
"$FORKLAUNCH_CLI" sync worker jobs --path .
"$FORKLAUNCH_CLI" init worker jobs --path . --language rust -t bullmq -D Jobs
grep -q 'variant = "worker-bullmq"' .forklaunch/manifest.toml
grep -q 'workerType = "bullmq"' .forklaunch/manifest.toml
cargo build --workspace
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory))')"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$TARGET_DIR/debug/api"
node "$SCRIPT_DIR/rust_bullmq_worker_runtime.mjs" "$TARGET_DIR/debug/jobs" stub
cp "$SCRIPT_DIR/fixtures/rust_worker_handler.rs" crates/jobs/src/handler.rs
cargo build -p jobs
node "$SCRIPT_DIR/rust_bullmq_worker_runtime.mjs" "$TARGET_DIR/debug/jobs" runtime
# Registered crates can live outside crates/: lifecycle commands must honor cratePath.
python3 - <<'PYTHON'
from pathlib import Path
source=Path('crates/jobs');target=Path('workers/team/jobs');target.parent.mkdir(parents=True)
source.rename(target)
p=target/'Cargo.toml';p.write_text(p.read_text().replace('../../.forklaunch/', '../../../.forklaunch/'))
for name in ['Cargo.toml','.forklaunch/manifest.toml']:
    p=Path(name);p.write_text(p.read_text().replace('crates/jobs','workers/team/jobs'))
p=target/'src/handler.rs';p.write_text(p.read_text()+'\npub const USER_OWNED_MARKER: &str = "preserved";\n')
PYTHON
"$FORKLAUNCH_CLI" sync worker jobs --path .
"$FORKLAUNCH_CLI" init worker jobs --path . --language rust -t bullmq -D Jobs
test ! -d crates/jobs
grep -q USER_OWNED_MARKER workers/team/jobs/src/handler.rs
"$FORKLAUNCH_CLI" openapi export --path .
"$FORKLAUNCH_CLI" sdk generate --path . --target all --from contract
node -e 'const fs=require("fs");const c=JSON.parse(fs.readFileSync(".forklaunch/contracts/jobs/contract.json"));if(c.routes.length)throw Error("Worker advertises application routes");'
"$FORKLAUNCH_CLI" change worker --path workers/team/jobs -t redis --confirm
"$FORKLAUNCH_CLI" change worker --path workers/team/jobs -t bullmq --confirm
cargo check -p jobs
"$FORKLAUNCH_CLI" change worker --path workers/team/jobs -N renamed-jobs --confirm
cargo check -p renamed-jobs
grep -q USER_OWNED_MARKER workers/team/renamed-jobs/src/handler.rs
"$FORKLAUNCH_CLI" delete worker renamed-jobs --path . --continue
test ! -d workers/team/renamed-jobs
echo 'PASS generated BullMQ worker runtime, service HTTP/MCP surfaces and worker lifecycle'
