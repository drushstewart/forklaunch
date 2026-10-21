#!/usr/bin/env bash
set -euo pipefail
# Run in the owned, already running CLI-generated Notes review application.
test -f scripts/check-mcp.mjs
test -f compose.yaml
fixture_override="$(mktemp "${TMPDIR:-/tmp}/notes-mcp.XXXXXX")"
cleanup() {
    fixture_status=$?
    rm -f "$fixture_override"
    docker compose up -d --no-build --no-deps notes >/dev/null 2>&1 || fixture_status=1
    exit "$fixture_status"
}
trap cleanup EXIT
cat > "$fixture_override" <<'YAML'
services:
  notes:
    environment:
      MCP_ENABLED: "false"
YAML
docker compose -f compose.yaml -f "$fixture_override" up -d --no-build --no-deps notes >/dev/null
check_primary() {
    docker compose exec -T auth node --input-type=module <<'JS'
import assert from 'node:assert/strict';
let healthy=false;
for(let i=0;i<30;i++) {
 try { healthy=(await fetch('http://notes:8000/health')).status===204; } catch {}
 if(healthy)break;
 await new Promise(r=>setTimeout(r,500));
}
assert.ok(healthy,'HTTP listener remains available');
assert.equal((await fetch('http://notes:8000/mcp')).status,404,'MCP absent from HTTP listener');
JS
}
check_primary
cat > "$fixture_override" <<'YAML'
services:
  notes:
    environment:
      MCP_ENABLED: "true"
      MCP_LISTENER: separate
      MCP_HOST: 0.0.0.0
      MCP_PORT: "8001"
      MCP_ALLOWED_HOSTS: notes,notes:8001
YAML
docker compose -f compose.yaml -f "$fixture_override" up -d --no-build --no-deps notes >/dev/null
check_primary
docker compose exec -T -e API_CHECK_URL=http://notes:8001 auth node --input-type=module < scripts/check-mcp.mjs
echo 'PASS Notes MCP disabled and separate-listener modes preserve HTTP and authenticated tool invocation'
