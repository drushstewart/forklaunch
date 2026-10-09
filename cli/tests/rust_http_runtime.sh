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
cargo build --workspace
run_cli openapi export --path .
run_cli sdk generate --path . --target typescript --from contract
"$TSC_BIN" --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir sdks/typescript/dist sdks/typescript/src/index.ts
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s=""; process.stdin.on("data", d => s+=d); process.stdin.on("end", () => console.log(JSON.parse(s).target_directory + "/debug/accounts"))')"
node --input-type=module - "$BINARY" <<'JS'
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createForkLaunchSdk } from './sdks/typescript/dist/index.js';
const service = spawn(process.argv[2], [], {
  env: { ...process.env, HOST: '127.0.0.1', SERVICE_HOST: '127.0.0.1', SERVICE_PORT: '0' },
  stdio: ['ignore', 'pipe', 'pipe']
});
const exited = once(service, 'exit');
let stderr = '';
service.stderr.on('data', chunk => stderr += chunk);
try {
  const baseUrl = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`startup timed out: ${stderr}`)), 15000);
    let output = '';
    service.stdout.on('data', chunk => {
      output += chunk;
      const match = output.match(/listening on (http:\/\/[^\s]+)/);
      if (match) { clearTimeout(timeout); resolve(match[1]); }
    });
    service.once('exit', code => { clearTimeout(timeout); reject(new Error(`service exited ${code}: ${stderr}`)); });
    service.once('error', error => { clearTimeout(timeout); reject(error); });
  });
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
  service.kill('SIGTERM');
  const [code, signal] = await exited;
  assert.equal(code, 0, `shutdown failed: ${signal} ${stderr}`);
  console.log('PASS generated TypeScript SDK against relocated Rust HTTP service and graceful shutdown');
} finally {
  if (service.exitCode === null && service.signalCode === null) {
    service.kill('SIGKILL');
    await exited;
  }
}
JS
