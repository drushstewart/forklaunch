#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-kafka-container-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-kafka-container-runtime/run.XXXXXX")"
NETWORK="forklaunch-rust-kafka-container-$$"
BROKER="$NETWORK-broker"
WORKER="$NETWORK-worker"
IMAGE="forklaunch-rust-kafka-container:smoke-$$"
cleanup() {
    docker rm -f "$WORKER" "$BROKER" >/dev/null 2>&1 || true
    docker network rm "$NETWORK" >/dev/null 2>&1 || true
    docker image rm "$IMAGE" >/dev/null 2>&1 || true
}
trap cleanup EXIT
run_cli init application container-app --path "$RUN_ROOT/app" --language rust -D 'Kafka container runtime' -A ForkLaunch -L MIT
run_cli init worker jobs --path "$RUN_ROOT/app" --language rust -t kafka -D Jobs
cp "$SCRIPT_DIR/fixtures/rust_kafka_worker_handler.rs" "$RUN_ROOT/app/crates/jobs/src/handler.rs"
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
docker build --progress=plain -t "$IMAGE" "$RUN_ROOT/relocated-app"
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
docker run --rm --env FORKLAUNCH_MODE=contract --env OTEL_EXPORTER_OTLP_PROTOCOL=invalid \
    "$IMAGE" /app/target/release/jobs
worker_command() {
    docker run --rm --network "$NETWORK" --env KAFKA_BROKERS=broker:9092 \
        --env WORKER_NAMESPACE=container-parity --env TENANT_ID=container-tenant \
        --env KAFKA_TRANSACTIONAL_ID=container-cli --env KAFKA_PUBLISHER_ID=container-enqueue \
        "$IMAGE" /app/target/release/jobs "$@"
}
worker_command --migrate
docker run --detach --name "$WORKER" --network "$NETWORK" --publish 127.0.0.1::8000 \
    --env KAFKA_BROKERS=broker:9092 --env WORKER_NAMESPACE=container-parity --env TENANT_ID=container-tenant \
    --env KAFKA_TRANSACTIONAL_ID=container-worker --env WORKER_HTTP_ADDR=0.0.0.0:8000 \
    --env WORKER_CONTROL_TOKEN=container-parity-control-token-32-chars \
    --health-cmd 'curl --fail --silent http://127.0.0.1:8000/health >/dev/null' \
    --health-interval 1s --health-timeout 3s --health-retries 30 \
    "$IMAGE" /app/target/release/jobs >/dev/null
healthy=false
for _ in $(seq 1 60); do
    if [ "$(docker inspect --format '{{.State.Health.Status}}' "$WORKER")" = healthy ]; then healthy=true; break; fi
    if [ "$(docker inspect --format '{{.State.Running}}' "$WORKER")" != true ]; then break; fi
    sleep 1
done
if [ "$healthy" != true ]; then docker logs "$WORKER"; exit 1; fi
worker_command --enqueue container-job '{}'
processed=false
for _ in $(seq 1 60); do
    if docker logs "$WORKER" 2>&1 | rg -q 'test-handler-started container-job'; then processed=true; break; fi
    sleep 1
done
test "$processed" = true
# A bounded inspection also loads the native client in a separate Linux process.
committed=false
for _ in $(seq 1 20); do
    if worker_command --peek 10 | python3 -c 'import json,sys; assert json.load(sys.stdin) == []'; then committed=true; break; fi
    sleep 1
done
test "$committed" = true
ADDRESS="$(docker port "$WORKER" 8000/tcp)"
curl --fail --silent --max-time 10 "http://$ADDRESS/ready" >/dev/null
curl --fail --silent --max-time 10 --request POST \
    --header 'Authorization: Bearer container-parity-control-token-32-chars' \
    "http://$ADDRESS/control/pause" >/dev/null
docker stop --time 15 "$WORKER" >/dev/null
test "$(docker inspect --format '{{.State.ExitCode}}' "$WORKER")" = 0
echo 'PASS generated Kafka Linux container, bundled librdkafka/OpenSSL, offline export, migration, enqueue, processing, inspection, health, controls and SIGTERM'
