#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then
        "$FORKLAUNCH_CLI" "$@"
    else
        cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"
    fi
}

# Allow an already generated fixture for local retries; CI generates its own.
if [ -n "${FORKLAUNCH_RUST_CONTAINER_APP:-}" ]; then
    APP_ROOT="$(cd "$FORKLAUNCH_RUST_CONTAINER_APP" && pwd)"
else
    mkdir -p "$SCRIPT_DIR/output/rust-container-runtime"
    RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-container-runtime/run.XXXXXX")"
    run_cli init application container-app --path "$RUN_ROOT/app" --language rust -D 'Container runtime' -A ForkLaunch -L MIT
    run_cli init service accounts --path "$RUN_ROOT/app" --language rust -d postgresql -D Accounts
    cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
    APP_ROOT="$RUN_ROOT/relocated-app"
fi

NATIVE_SERVICE="${FORKLAUNCH_RUST_CONTAINER_SERVICE:-accounts}"
NATIVE_DOCKERFILE="${FORKLAUNCH_RUST_CONTAINER_DOCKERFILE:-Dockerfile}"

CONTAINER="forklaunch-rust-runtime-$$"
IMAGE="forklaunch-rust-runtime:smoke-$$"
cleanup() {
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    docker image rm "$IMAGE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker build --progress=plain -f "$APP_ROOT/$NATIVE_DOCKERFILE" -t "$IMAGE" "$APP_ROOT"
docker run --detach --name "$CONTAINER" --publish 127.0.0.1::8000 \
    --env HOST=0.0.0.0 --env SERVICE_PORT=8000 \
    --health-cmd 'curl --fail --silent http://127.0.0.1:8000/health >/dev/null' \
    --health-interval 1s --health-timeout 3s --health-retries 30 \
    "$IMAGE" "/app/target/release/$NATIVE_SERVICE"

healthy=false
for _ in $(seq 1 60); do
    status="$(docker inspect --format '{{.State.Health.Status}}' "$CONTAINER")"
    if [ "$status" = healthy ]; then healthy=true; break; fi
    if [ "$(docker inspect --format '{{.State.Running}}' "$CONTAINER")" != true ]; then break; fi
    sleep 1
done
if [ "$healthy" != true ]; then
    docker logs "$CONTAINER"
    echo 'FAIL generated Rust container failed its health check' >&2
    exit 1
fi
ADDRESS="$(docker port "$CONTAINER" 8000/tcp)"
curl --fail --silent --max-time 10 "http://$ADDRESS/health" | python3 -c 'import json,sys; assert json.load(sys.stdin)["status"] == "ok"'
curl --fail --silent --max-time 10 "http://$ADDRESS/openapi.json" | python3 -c 'import json,sys; assert "/health" in json.load(sys.stdin)["paths"]'
node --input-type=module - "$SCRIPT_DIR/rust_mcp_probe.mjs" "http://$ADDRESS" <<'JS'
import {pathToFileURL} from 'node:url';
const {probeMcp}=await import(pathToFileURL(process.argv[2]));
await probeMcp(process.argv[3]);
JS
docker stop --time 10 "$CONTAINER" >/dev/null
test "$(docker inspect --format '{{.State.ExitCode}}' "$CONTAINER")" = 0
echo 'PASS relocated Rust container build, health/OpenAPI/MCP, and SIGTERM shutdown'
