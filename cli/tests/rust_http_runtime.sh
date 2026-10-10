#!/usr/bin/env bash
set -euo pipefail
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
run_cli() {
    if [ -n "${FORKLAUNCH_CLI:-}" ]; then
        "$FORKLAUNCH_CLI" "$@"
    else
        cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"
    fi
}
mkdir -p output/rust-http-runtime
cd "$(mktemp -d output/rust-http-runtime/run.XXXXXX)"
run_cli init application runtime-app --path runtime-app --language rust -D 'HTTP runtime' -A ForkLaunch -L MIT
run_cli init service accounts --path runtime-app --language rust -d postgresql -D Accounts
run_cli init router users --path runtime-app/crates/accounts --language rust
# Relocation catches accidental dependencies on the CLI build machine's checkout.
cp -R runtime-app relocated-app
cd relocated-app
cp "$SCRIPT_DIR/fixtures/rust_sdk_transport.rs" crates/accounts/src/routes/transport_fixture.rs
python3 - <<'PYTHON'
from pathlib import Path
p = Path('crates/accounts/src/routes/mod.rs')
s = p.read_text()
s = 'pub mod transport_fixture;\n' + s
s = s.replace('pub fn contracts() -> Vec<RouteContract> {\n    vec![', 'pub fn contracts() -> Vec<RouteContract> {\n    let mut contracts = transport_fixture::contracts();\n    contracts.extend(vec![')
s = s.replace('::contract(),\n    ]', '::contract(),\n    ]);\n    contracts')
s = s.replace('pub fn register_handlers(router: &mut LocalRouter, state: AppState) {', 'pub fn register_handlers(router: &mut LocalRouter, state: AppState) {\n    transport_fixture::register(router);')
p.write_text(s)
PYTHON
cargo build --workspace
run_cli openapi export --path .
NODE_TYPES_ROOT="${NODE_TYPES_ROOT:-$(dirname "$TSC_BIN")/../@types}"
"$TSC_BIN" --strict --skipLibCheck --target ES2022 --module NodeNext --moduleResolution NodeNext \
    --types node --typeRoots "$NODE_TYPES_ROOT" --outDir "$PWD/generated-wire-fixtures" \
    "$SCRIPT_DIR/fixtures/typescript_sdk_server.mts"
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s=""; process.stdin.on("data", d => s+=d); process.stdin.on("end", () => console.log(JSON.parse(s).target_directory + "/debug/accounts"))')"
cat > generated-empty-response-usage.ts <<'TS'
import { createForkLaunchSdk } from './sdks/typescript/src/index';
async function useResponses(client = createForkLaunchSdk()) {
  const deleted = await client.accounts.users.deleteUser({ params: { id: 'welcome' } });
  if (deleted.code === 204) { const empty: undefined = deleted.response; void empty; }
  const emptyReply = await client.accounts.empty.getEmpty();
  if (emptyReply.code === 200) { const empty: undefined = emptyReply.response; void empty; }
  const nullReply = await client.accounts.nullJson.getNull();
  if (nullReply.code === 200) { const value: null = nullReply.response; void value; }
}
void useResponses;
TS
for SDK_SOURCE in contract openapi; do
run_cli sdk generate all --path . --from "$SDK_SOURCE"
"$TSC_BIN" --noEmit --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM generated-empty-response-usage.ts
"$TSC_BIN" --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
cat >> sdks/rust/accounts-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
TOML
mkdir -p sdks/rust/accounts-sdk/examples
cp "$SCRIPT_DIR/fixtures/rust_sdk_http.rs" sdks/rust/accounts-sdk/examples/http_roundtrip.rs
cargo build -p accounts-sdk --example http_roundtrip
node --input-type=module - "$BINARY" "$(dirname "$BINARY")/examples/http_roundtrip" "$PWD/generated-wire-fixtures/typescript_sdk_server.mjs" <<'JS'
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createForkLaunchSdk } from './sdks/typescript/dist/index.js';
const children = [];
async function startServer(program, args = []) {
  const process = spawn(program, args, {
    env: { ...globalThis.process.env, HOST: '127.0.0.1', SERVICE_HOST: '127.0.0.1', SERVICE_PORT: '0' },
    stdio: ['ignore', 'pipe', 'pipe']
  });
  const exited = once(process, 'exit');
  // Keep a rejection handled even if startup fails before shutdown is awaited.
  exited.catch(() => {});
  const child = { process, exited, stderr: '' };
  children.push(child);
  process.stderr.on('data', chunk => child.stderr += chunk);
  child.baseUrl = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`startup timed out: ${child.stderr}`)), 15000);
    let output = '';
    process.stdout.on('data', chunk => {
      output += chunk;
      const match = output.match(/listening on (http:\/\/[^\s]+)/);
      if (match) { clearTimeout(timeout); resolve(match[1]); }
    });
    process.once('exit', code => { clearTimeout(timeout); reject(new Error(`service exited ${code}: ${child.stderr}`)); });
    process.once('error', error => { clearTimeout(timeout); reject(error); });
  });
  return child;
}
async function verifyRustClient(baseUrl) {
  const client = spawn(process.argv[3], [baseUrl], { stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  client.stdout.on('data', chunk => output += chunk);
  client.stderr.on('data', chunk => output += chunk);
  const deadline = setTimeout(() => client.kill('SIGKILL'), 15000);
  try {
    const [code, signal] = await once(client, 'exit');
    assert.equal(code, 0, `Rust SDK failed: ${signal} ${output}`);
    assert.match(output, /PASS Rust SDK live HTTP roundtrip/);
  } finally {
    clearTimeout(deadline);
    if (client.exitCode === null && client.signalCode === null) client.kill('SIGKILL');
  }
}
async function stopServer(child) {
  child.process.kill('SIGTERM');
  const deadline = setTimeout(() => child.process.kill('SIGKILL'), 10000);
  try {
    const [code, signal] = await child.exited;
    assert.equal(code, 0, `shutdown failed: ${signal} ${child.stderr}`);
  } finally { clearTimeout(deadline); }
}
try {
  const service = await startServer(process.argv[2]);
  const baseUrl = service.baseUrl;
  const sdk = createForkLaunchSdk({ baseUrl });
  const health = await sdk.accounts.health.getHealth();
  assert.equal(health.code, 200);
  assert.equal(health.response.status, 'ok');
  const users = await sdk.accounts.users.listUsers();
  assert.equal(users.code, 200);
  assert.equal(users.response.records[0].id, 'welcome');
  assert.equal((await fetch(`${baseUrl}/docs`)).headers.get('content-type'), 'text/html; charset=utf-8');
  assert.equal((await fetch(`${baseUrl}/missing`)).status, 404);
  assert.equal((await fetch(`${baseUrl}/health`, { method: 'POST' })).status, 405);
  assert.equal(await (await fetch(`${baseUrl}/health`, { method: 'HEAD' })).text(), '');
  const largeHeader = await fetch(`${baseUrl}/health?label=hello%20world`, { headers: { 'x-large': 'x'.repeat(8192) } });
  assert.equal(largeHeader.status, 200);
  const responses = await Promise.all(Array.from({ length: 20 }, () => sdk.accounts.health.getHealth()));
  assert(responses.every(response => response.code === 200));
  const deleted = await sdk.accounts.users.deleteUser({ params: { id: 'welcome' } });
  assert.equal(deleted.code, 204);
  assert.equal(deleted.response, undefined);
  assert.equal((await sdk.accounts.empty.getEmpty()).response, undefined);
  assert.equal((await sdk.accounts.health.headHealth()).response, undefined);
  assert.equal((await sdk.accounts.nullJson.getNull()).response, null);
  await verifyRustClient(baseUrl);
  await stopServer(service);
  console.log('PASS generated TypeScript and Rust SDKs against relocated Rust HTTP service and graceful shutdown');
  const typescript = await startServer(process.execPath, [process.argv[4]]);
  await verifyRustClient(typescript.baseUrl);
  const tsSdk = createForkLaunchSdk({ baseUrl: typescript.baseUrl });
  const tsHealth = await tsSdk.accounts.health.getHealth();
  assert.deepEqual(tsHealth.response, health.response);
  assert.deepEqual((await tsSdk.accounts.users.listUsers()).response, users.response);
  assert.equal((await tsSdk.accounts.users.deleteUser({ params: { id: 'welcome' } })).response, undefined);
  assert.equal((await tsSdk.accounts.empty.getEmpty()).response, undefined);
  assert.equal((await tsSdk.accounts.health.headHealth()).response, undefined);
  assert.equal((await tsSdk.accounts.nullJson.getNull()).response, null);
  await stopServer(typescript);
  console.log('PASS generated Rust and TypeScript SDKs against compiled TypeScript HTTP wire fixture');
} finally {
  for (const child of children) {
    if (child.process.exitCode === null && child.process.signalCode === null) {
      child.process.kill('SIGKILL');
      await child.exited.catch(() => {});
    }
  }
}
JS
done
