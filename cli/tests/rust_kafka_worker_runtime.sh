#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-kafka-worker-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-kafka-worker-runtime/run.XXXXXX")"
KAFKA_CONTAINER="forklaunch-rust-kafka-worker-test-$$"
REDIS_CONTAINER="forklaunch-rust-kafka-signals-test-$$"
OWN_KAFKA=false
OWN_REDIS=false
cleanup() {
    if [ "$OWN_KAFKA" = true ]; then docker rm -f "$KAFKA_CONTAINER" >/dev/null 2>&1 || true; fi
    if [ "$OWN_REDIS" = true ]; then docker rm -f "$REDIS_CONTAINER" >/dev/null 2>&1 || true; fi
}
trap cleanup EXIT
if [ -z "${FORKLAUNCH_TEST_KAFKA_BROKERS:-}" ]; then
    OWN_KAFKA=true
    TASK_KAFKA_PORT=$(python3 - <<'PYTHON'
import socket
with socket.socket() as sock:
    sock.bind(('127.0.0.1', 0))
    print(sock.getsockname()[1])
PYTHON
)
    export FORKLAUNCH_TEST_KAFKA_BROKERS="127.0.0.1:$TASK_KAFKA_PORT"
    docker run --detach --name "$KAFKA_CONTAINER" --memory 1536m --cpus 2 \
        --publish "127.0.0.1:$TASK_KAFKA_PORT:9092" \
        --tmpfs /var/lib/redpanda/data:rw,size=512m,mode=1777 \
        redpandadata/redpanda@sha256:218469e5d088757bb2c3ff4c5e272f7eebdc4e94c933e6e15aff10b845cbcd07 \
        redpanda start --overprovisioned --smp 1 --memory 768M --reserve-memory 0M \
        --node-id 0 --check=false --kafka-addr 0.0.0.0:9092 \
        --advertise-kafka-addr "$FORKLAUNCH_TEST_KAFKA_BROKERS"
    for _ in $(seq 1 60); do
        if docker exec "$KAFKA_CONTAINER" rpk cluster health --exit-when-healthy >/dev/null 2>&1; then break; fi
        sleep 1
    done
fi
if [ -z "${FORKLAUNCH_TEST_REDIS_URL:-}" ]; then
    OWN_REDIS=true
    docker run --detach --name "$REDIS_CONTAINER" --publish 127.0.0.1::6379 \
        --log-driver none redis:7-alpine redis-server --save '' --appendonly no
    for _ in $(seq 1 60); do
        if docker exec "$REDIS_CONTAINER" redis-cli ping >/dev/null 2>&1; then break; fi
        sleep 1
    done
    export FORKLAUNCH_TEST_REDIS_URL="redis://$(docker port "$REDIS_CONTAINER" 6379/tcp)/"
fi
export FORKLAUNCH_TEST_WORKER_NAMESPACE="fl_kafka_$(date +%s)_$$"
run_cli init application worker-app --path "$RUN_ROOT/app" --language rust -D 'Kafka worker runtime' -A ForkLaunch -L MIT
run_cli init worker dry-worker --path "$RUN_ROOT/app" --language rust -t kafka --dryrun
test ! -d "$RUN_ROOT/app/crates/dry-worker"
test ! -d "$RUN_ROOT/app/.forklaunch/blueprint-rs"
run_cli init worker jobs --path "$RUN_ROOT/app" --language rust -t kafka -D Jobs
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
printf '\npub const USER_OWNED_MARKER: &str = "preserved";\n' >> crates/jobs/src/handler.rs
printf '\n[package.metadata.user]\nmarker = "preserved"\n' >> crates/jobs/Cargo.toml
run_cli sync worker jobs --path .
run_cli init worker jobs --path . --language rust -t kafka -D Jobs
grep -q USER_OWNED_MARKER crates/jobs/src/handler.rs
grep -q 'variant = "worker-kafka"' .forklaunch/manifest.toml
grep -q 'queue = "kafka"' .forklaunch/manifest.toml
grep -q WORKER_KAFKA_BROKERS docker-compose.yaml
cargo build --workspace
run_cli openapi export --path .
run_cli sdk generate --path . --target all --from contract
python3 - <<'PYTHON'
import json
from pathlib import Path
assert json.loads(Path('.forklaunch/contracts/jobs/contract.json').read_text())['routes']==[]
assert json.loads(Path('.forklaunch/openapi/jobs/openapi.json').read_text())['paths']=={}
PYTHON
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/jobs"))')"
node "$SCRIPT_DIR/rust_worker_control_service.mjs" "$BINARY"
node "$SCRIPT_DIR/rust_kafka_worker_runtime.mjs" "$BINARY" stub
cp "$SCRIPT_DIR/fixtures/rust_kafka_worker_handler.rs" crates/jobs/src/handler.rs
printf '\npub const USER_OWNED_MARKER: &str = "preserved";\n' >> crates/jobs/src/handler.rs
cargo build -p jobs
node "$SCRIPT_DIR/rust_kafka_worker_runtime.mjs" "$BINARY" runtime
cp -R "$RUN_ROOT/relocated-app" "$RUN_ROOT/lifecycle-app"
run_cli change worker --path "$RUN_ROOT/lifecycle-app/crates/jobs" -N email-jobs --confirm
grep -q USER_OWNED_MARKER "$RUN_ROOT/lifecycle-app/crates/email-jobs/src/handler.rs"
test ! -d "$RUN_ROOT/lifecycle-app/sdks/rust/jobs-sdk"
run_cli sdk generate --path "$RUN_ROOT/lifecycle-app" --target all --from contract --check
run_cli change worker --path "$RUN_ROOT/lifecycle-app/crates/email-jobs" -t redis --confirm
run_cli change worker --path "$RUN_ROOT/lifecycle-app/crates/email-jobs" -t kafka --confirm
grep -q USER_OWNED_MARKER "$RUN_ROOT/lifecycle-app/crates/email-jobs/src/handler.rs"
grep -q 'marker = "preserved"' "$RUN_ROOT/lifecycle-app/crates/email-jobs/Cargo.toml"
cargo test --manifest-path "$RUN_ROOT/lifecycle-app/Cargo.toml" --workspace
run_cli delete worker email-jobs --path "$RUN_ROOT/lifecycle-app" --continue
test ! -d "$RUN_ROOT/lifecycle-app/crates/email-jobs"
test ! -d "$RUN_ROOT/lifecycle-app/sdks/rust/email-jobs-sdk"
echo 'PASS generated Kafka worker source preservation, relocation, rename, backend conversion, and delete'
