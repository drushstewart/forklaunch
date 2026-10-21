#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
export CARGO_INCREMENTAL=0
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
export FORKLAUNCH_SKIP_FORMAT=1
export FORKLAUNCH_FRAMEWORK_SNAPSHOT=1
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-0}" != 1 ]; then
    exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "$SCRIPT_DIR/rust_kafka_container_runtime.sh"
fi
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-kafka-container-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-kafka-container-runtime/run.XXXXXX")"
if [ -n "$FORKLAUNCH_CLI" ]; then
    cp "$FORKLAUNCH_CLI" "$RUN_ROOT/forklaunch"
    FORKLAUNCH_CLI="$RUN_ROOT/forklaunch"
    shasum -a 256 "$FORKLAUNCH_CLI"
fi
PASSED=false
NETWORK="forklaunch-rust-kafka-container-$$"
BROKER="$NETWORK-broker"
WORKER="$NETWORK-worker"
IMAGE="forklaunch-rust-kafka-container:smoke-$$"
cleanup() {
    if [ "$PASSED" != true ]; then
        docker logs "$BROKER" > "$RUN_ROOT/kafka-broker-failure.log" 2>&1 || true
        docker logs "$WORKER" > "$RUN_ROOT/kafka-worker-failure.log" 2>&1 || true
    fi
    docker rm -f "$WORKER" "$BROKER" "$NETWORK-command" >/dev/null 2>&1 || true
    docker network rm "$NETWORK" >/dev/null 2>&1 || true
    docker image rm "$IMAGE" >/dev/null 2>&1 || true
    if [ "$PASSED" = true ] && [ "${FORKLAUNCH_KEEP_FIXTURE:-0}" != 1 ]; then rm -rf "$RUN_ROOT"; fi
}
trap cleanup EXIT
run_cli init application container-app --path "$RUN_ROOT/app" --database postgresql --validator zod --formatter prettier --linter eslint --http-framework express --runtime node --test-framework vitest -D 'Kafka container runtime' -A ForkLaunch -L MIT
run_cli init worker jobs --path "$RUN_ROOT/app" --language rust --consumer-only -t kafka -D Jobs
cp "$SCRIPT_DIR/fixtures/rust_kafka_container_handler.rs" "$RUN_ROOT/app/src/jobs/domain/services/handler.rs"
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cargo generate-lockfile --manifest-path "$RUN_ROOT/relocated-app/Cargo.toml"
docker build --progress=plain -f "$RUN_ROOT/relocated-app/Dockerfile.rust" -t "$IMAGE" "$RUN_ROOT/relocated-app"
docker network create "$NETWORK" >/dev/null
docker run --detach --name "$BROKER" --network "$NETWORK" --network-alias broker \
    --memory 1536m --cpus 2 --tmpfs /var/lib/redpanda/data:rw,size=512m,mode=1777 \
    redpandadata/redpanda@sha256:218469e5d088757bb2c3ff4c5e272f7eebdc4e94c933e6e15aff10b845cbcd07 \
    redpanda start --overprovisioned --smp 1 --memory 768M --reserve-memory 0M \
    --node-id 0 --check=false --kafka-addr 0.0.0.0:9092 --advertise-kafka-addr broker:9092 >/dev/null
healthy=false
for _ in $(seq 1 60); do
    if docker exec "$BROKER" rpk cluster health --exit-when-healthy >/dev/null 2>&1; then healthy=true; break; fi
    sleep 1
done
test "$healthy" = true
# No broker or tenant configuration is needed to export offline artifacts.
# The nonroot runtime image is immutable; offline exports use explicit writable
# output locations instead of trying to create application files under /app.
docker run --rm --env FORKLAUNCH_MODE=contract --env OTEL_EXPORTER_OTLP_PROTOCOL=invalid \
    --env FORKLAUNCH_CONTRACT_OUTPUT=/tmp/contract.json \
    --env FORKLAUNCH_OPENAPI_OUTPUT=/tmp/openapi.json \
    "$IMAGE" sh -ec '/app/bin/jobs; cat "$FORKLAUNCH_CONTRACT_OUTPUT"; printf "\n"; cat "$FORKLAUNCH_OPENAPI_OUTPUT"' | \
    python3 -c 'import json,sys; decoder=json.JSONDecoder(); raw=sys.stdin.read(); contract,offset=decoder.raw_decode(raw); openapi,end=decoder.raw_decode(raw[offset:].lstrip()); assert isinstance(contract.get("routes"),list); assert openapi.get("openapi","").startswith("3."); assert not raw[offset:].lstrip()[end:].strip()'
python3 "$SCRIPT_DIR/fixtures/rust_kafka_container_probe.py" "$RUN_ROOT" "$IMAGE" "$NETWORK" "$BROKER" "$WORKER"
PASSED=true
echo 'PASS generated Kafka Linux container, bundled librdkafka/OpenSSL, offline export, encrypted broker payload, rejected configuration/input, processing, inspection, health, controls and SIGTERM'
