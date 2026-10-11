#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
mkdir -p "$SCRIPT_DIR/output/rust-capabilities-compose"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-capabilities-compose/run.XXXXXX")"
"$FORKLAUNCH_CLI" init application capability-compose --path "$RUN_ROOT/app" --language rust -D Capabilities -A ForkLaunch -L MIT
"$FORKLAUNCH_CLI" init service messages --path "$RUN_ROOT/app" --language rust -d postgresql -D Messages
cd "$RUN_ROOT/app"
for feature in email sms whatsapp voice payments; do "$FORKLAUNCH_CLI" infra add messages "$feature" --path .; done
mkdir -p crates/messages/examples
cp "$SCRIPT_DIR/fixtures/rust_capabilities_gateway.rs" crates/messages/examples/gateway.rs
python3 - <<'PY'
from pathlib import Path
p=Path('Dockerfile');p.write_text(p.read_text().replace('cargo build --release --workspace --bins','cargo build --release --workspace --bins --examples'))
PY
# Use the checked-in gateway implementation, so this also detects protocol drift.
cat > compose.fixture.yaml <<YAML
services:
  messages:
    ports: !override
      - "127.0.0.1::8000"
  gateway-mock:
    image: node:25-trixie-slim
    command: ["node", "/mock/server.mjs"]
    environment:
      MOCK_VOICE_CALL_MS: "5000"
    volumes:
      - "$REPO_ROOT/framework/core/bin/gateway-mock:/mock:ro"
    ports: !override
      - "127.0.0.1::8080"
YAML
compose() { docker compose -p "fl-native-capabilities-$$" -f docker-compose.yaml -f compose.fixture.yaml "$@"; }
trap 'compose down --volumes --remove-orphans >/dev/null 2>&1 || true' EXIT
compose up --build --detach --wait --wait-timeout 180
compose run --rm --no-deps messages ./target/release/examples/gateway
export FORKLAUNCH_TEST_SERVICE_BASE="http://$(compose port messages 8000)"
export FORKLAUNCH_TEST_GATEWAY_BASE="http://$(compose port gateway-mock 8080)"
node --input-type=module - "$SCRIPT_DIR" <<'JS'
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const {probeMcp}=await import(pathToFileURL(`${process.argv[2]}/rust_mcp_probe.mjs`));
await probeMcp(process.env.FORKLAUNCH_TEST_SERVICE_BASE);
const gateway=process.env.FORKLAUNCH_TEST_GATEWAY_BASE;
for(const feature of ['email','sms','whatsapp','voice','payments']) {
 const response=await fetch(`${gateway}/__mock/events/${feature}`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({type:`${feature}.fixture`,data:{}})});
 assert.equal(response.status,200);const result=await response.json();assert.equal(result.status,200,JSON.stringify(result));
}
const requests=await (await fetch(`${gateway}/__mock/requests`)).json();
for(const feature of ['email','sms','whatsapp','voice','payments'])assert.ok(requests.some(request=>request.feature===feature),`Missing signed ${feature} request`);
console.log('PASS Linux Compose HTTP/MCP startup, native signed outbound clients and gateway-to-service signed callbacks for all five capabilities');
JS
