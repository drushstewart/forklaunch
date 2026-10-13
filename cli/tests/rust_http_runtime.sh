#!/usr/bin/env bash
set -euo pipefail
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
export CROSS_FETCH_PATH="${CROSS_FETCH_PATH:-$(dirname "$TSC_BIN")/../cross-fetch}"
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
cp "$SCRIPT_DIR/rust_mcp_probe.mjs" rust_mcp_probe.mjs
cp "$SCRIPT_DIR/fixtures/rust_sdk_transport.rs" crates/accounts/src/routes/transport_fixture.rs
python3 - <<'PYTHON'
from pathlib import Path
manifest = Path('crates/accounts/Cargo.toml')
manifest.write_text(manifest.read_text().replace('[dependencies]\n', '[dependencies]\nserde_json = \"1.0\"\n'))
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
  const several = await client.accounts.status.getStatus({ params: { mode: 'created' } });
  if (several.code === 200) { const value: string = several.response.displayName; void value; }
  if (several.code === 201) { const value: string = several.response.createdAt; void value; }
  if (several.code === 202) { const value: string = several.response.jobId; void value; }
  if (several.code === 204 || several.code === 205) { const value: undefined = several.response; void value; }
  if (several.code === 400) { const value: string = several.response.errorCode; const mime: 'application/problem+json' = several.contentType; void value; void mime; }
  if (several.code === 409) { const value: number = several.response.retryAfter; void value; }
  const textReply = await client.accounts.text.getText();
  if (textReply.code === 200) { const value: string = textReply.response; void value; }
  const binaryReply = await client.accounts.binary.getBinary();
  if (binaryReply.code === 200) { const value: Uint8Array = binaryReply.response; void value; }
  const shapeReply = await client.accounts.shape.getShape({ params: { mode: 'null' } });
  if (shapeReply.code === 200) {
    const label: string | null = shapeReply.response.label;
    const number: number | null = shapeReply.response.numbers.one;
    const metadata: { id: string } | null = shapeReply.response.metadata;
    const extra: unknown = shapeReply.response.open.extra;
    void label; void number; void metadata; void extra;
  }
  await client.accounts.text.uploadText({ body: 'hello' });
  await client.accounts.binary.uploadBinary({ body: new Uint8Array([0, 255]) });
  // @ts-expect-error Binary bodies require bytes, not a JSON string.
  await client.accounts.binary.uploadBinary({ body: 'wrong' });
  // @ts-expect-error Text bodies require a string.
  await client.accounts.text.uploadText({ body: { wrong: true } });
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
import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createRequire } from 'node:module';
import { createForkLaunchSdk, forkLaunchQueryOptions, forkLaunchMutationOptions, ForkLaunchSdkHttpError } from './sdks/typescript/dist/index.js';
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
async function verifyStatusVariants(sdk, baseUrl) {
  for(const fetcher of [fetch,createRequire(import.meta.url)(process.env.CROSS_FETCH_PATH)]) {
    const bounded=createForkLaunchSdk({baseUrl,fetch:fetcher,maxMultipartResponseBytes:16});
    await assert.rejects(()=>bounded.accounts.multipartResponse.getMultipartResponse({params:{mode:'valid'}}),/exceeds byte limit/);
    const invalid=createForkLaunchSdk({baseUrl,fetch:fetcher,maxMultipartResponseBytes:0});
    await assert.rejects(()=>invalid.accounts.multipartResponse.getMultipartResponse({params:{mode:'valid'}}),/byte limit/);
    const adapted=createForkLaunchSdk({baseUrl,fetch:fetcher});
    const result=await adapted.accounts.multipartResponse.getMultipartResponse({params:{mode:'valid'}});
    assert.deepEqual(result.response.getAll('label'),['hello 水','again']);
    const abort=new AbortController();abort.abort();
    await assert.rejects(()=>adapted.accounts.multipartResponse.getMultipartResponse({params:{mode:'valid'},signal:abort.signal}));
  }

  const multipart=(await sdk.accounts.multipartResponse.getMultipartResponse({params:{mode:'valid'}})).response;
  assert.ok(multipart instanceof FormData);
  assert.deepEqual(multipart.getAll('label'),['hello 水','again']);
  const files=multipart.getAll('file');assert.equal(files.length,2);
  assert.equal(files[0].name,'data.bin');assert.equal(files[1].name,'empty.bin');
  assert.deepEqual([...new Uint8Array(await files[0].arrayBuffer())],[0,255,128,13,10]);
  assert.equal(files[1].size,0);
  assert.equal([...(await sdk.accounts.multipartResponse.getMultipartResponse({params:{mode:'empty'}})).response].length,0);
  for(const mode of ['wrong','missing','invalid','truncated','many']) await assert.rejects(()=>sdk.accounts.multipartResponse.getMultipartResponse({params:{mode}}));

  for(const key of ['$serde_json::private::Number','$serde_json::private::RawValue']) {
    const body={value:{[key]:'123',nested:[{[key]:{literal:true}}]}};
    assert.deepEqual((await sdk.accounts.jsonEcho.echoJson({body})).response,body);
  }

  const query=forkLaunchQueryOptions(['accounts','status','queued'],sdk.accounts.status.getStatus,{params:{mode:'queued'}});
  assert.equal((await query.queryFn({signal:new AbortController().signal})).response.jobId,'queued-1');
  const mutation=forkLaunchMutationOptions(['accounts','status'],sdk.accounts.status.getStatus);
  assert.equal((await mutation.mutationFn({params:{mode:'empty'}})).response,undefined);
  await assert.rejects(mutation.mutationFn({params:{mode:'invalid'}}),error=>error instanceof ForkLaunchSdkHttpError&&error.result.status===400&&error.result.response.errorCode==='invalid_mode');

  assert.equal((await sdk.accounts.nullable.getNullable({ params: { mode: 'null' } })).response, null);
  assert.deepEqual((await sdk.accounts.nullable.getNullable({ params: { mode: 'full' } })).response, { name: 'nullable' });
  for (const [mode, expected] of Object.entries({"named": {"name": "Rust"}, "counted": {"count": 42}, "boolean": true, "string": "union", "integer": 7})) {
    assert.deepEqual((await sdk.accounts.union.getUnion({ params: { mode } })).response, expected);
  }
  assert.deepEqual((await sdk.accounts.unionList.getUnionList()).response, [{ name: "nested" }, "two", 3]);
  assert.equal((await sdk.accounts.text.uploadText({ body: '{raw}\n🦀 & text' })).response, '{raw}\n🦀 & text');
  assert.equal((await sdk.accounts.text.uploadText({ body: '' })).response, '');
  assert.deepEqual(Array.from((await sdk.accounts.binary.uploadBinary({ body: new Uint8Array() })).response), []);
  const uploaded = await sdk.accounts.binary.uploadBinary({ body: new Uint8Array([99, 0, 255, 13, 10, 42, 99]).subarray(1, 6) });
  assert.deepEqual([...uploaded.response], [0, 255, 13, 10, 42]);
  for (const empty of [false, true]) {
    const form = new FormData();
    if (!empty) {
      form.append('label', 'hello 水');
      form.append('file', new Blob([new Uint8Array([0,255,1,13,10])]), 'data.bin');
      form.append('file', new Blob([]), 'empty.bin');
    }
    const uploaded = await sdk.accounts.multipart.uploadMultipart({ body: form, headers: { 'Content-Type': 'multipart/form-data; boundary=incorrect' } });
    assert.equal(uploaded.code, 200);
    assert.match(uploaded.response.contentType, /^multipart\/form-data; boundary=/);
    assert.ok(!uploaded.response.contentType.includes('incorrect'));
    const parsed = await new Response(new Uint8Array(uploaded.response.bytes), { headers: { 'content-type': uploaded.response.contentType } }).formData();
    assert.equal([...parsed.keys()].length, empty ? 0 : 3);
    if (!empty) {
      assert.equal(parsed.get('label'), 'hello 水');
      const files = parsed.getAll('file');
      assert.equal(files[0].name, 'data.bin');
      assert.deepEqual([...new Uint8Array(await files[0].arrayBuffer())], [0,255,1,13,10]);
      assert.equal(files[1].size, 0);
    }
  }
  assert.deepEqual((await sdk.accounts.jsonEcho.echoJson({ body: { value: [1, null, true] } })).response, { value: [1, null, true] });
  for (const [mode, expected] of [["null", {"label": null, "numbers": {"one": 1, "missing": null}, "metadata": null, "open": {"id": "open", "extra": {"nested": true}}, "constrained": {"count": 2, "extra": "text"}, "additional_properties": "explicit", "unknownExtra": [1, false], "any": 7, "list": [1, "two", null]}], ["full", {"label": "present", "numbers": {"one": 1, "missing": null}, "metadata": {"id": "meta"}, "open": {"id": "open", "extra": {"nested": true}}, "constrained": {"count": 2, "extra": "text"}, "additional_properties": "explicit", "unknownExtra": [1, false], "any": "seven", "list": [1, "two", null]}]]) {
    assert.deepEqual((await sdk.accounts.shape.getShape({ params: { mode } })).response, expected);
  }
  assert.deepEqual((await sdk.accounts.collision.getCollision()).response, {"fooBar": {"first": "one"}, "foo_bar": {"second": 2}, "foo_bar2": {"third": true}, "foo_bar_2": {"fourth": "four"}, "type": "kind", "type_": 9, "gen": "future", "_": "underscore", "quote\"\\": "escaped"});
  const text = await sdk.accounts.text.getText();
  assert.equal(text.response, 'Hello\nRust 🦀');
  assert.equal(text.contentType, 'text/plain');
  const binary = await sdk.accounts.binary.getBinary();
  assert.ok(binary.response instanceof Uint8Array);
  assert.deepEqual([...binary.response], [0, 255, 13, 10, 42]);
  assert.equal(binary.contentType, 'application/octet-stream');
  assert.deepEqual((await sdk.accounts.unknown.getUnknown()).response, [1, 'two', { three: true }]);
  for (const [mode, code, expected] of [
    ['existing', 200, { id: 'existing', displayName: 'Existing' }],
    ['created', 201, { id: 'created', createdAt: '2026-10-09T00:00:00Z' }],
    ['queued', 202, { jobId: 'queued-1' }], ['empty', 204, undefined],
    ['reset', 205, undefined], ['invalid', 400, { errorCode: 'invalid_mode', message: 'Unknown mode' }],
    ['conflict', 409, { errorCode: 'conflict', retryAfter: 30 }]
  ]) {
    const response = await sdk.accounts.status.getStatus({ params: { mode } });
    assert.equal(response.code, code);
    assert.deepEqual(response.response, expected);
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
  await probeMcp(service.baseUrl,['getText','getBinary']);
  const mcpText=await mcpRpc(service.baseUrl,'tools/call',{name:'getText',arguments:{}});assert.equal(mcpText.content[0].text,'Hello\nRust 🦀');
  const mcpBinary=await mcpRpc(service.baseUrl,'tools/call',{name:'getBinary',arguments:{}});assert.deepEqual([...Buffer.from(mcpBinary.content[0].resource.blob,'base64')],[0,255,13,10,42]);

  for(const key of ['$serde_json::private::Number','$serde_json::private::RawValue']) {
    const body={value:{[key]:'123',nested:[{[key]:{literal:true}}]}};
    const result=await mcpRpc(service.baseUrl,'tools/call',{name:'echoJson',arguments:{body}});
    assert.deepEqual(result.structuredContent,body);
  }
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
  await verifyStatusVariants(sdk, service.baseUrl);
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
  await verifyStatusVariants(tsSdk, typescript.baseUrl);
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

# Compile native generators against the same live service contracts and wire cases.
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-typescript --example multipart_clients -- "$PWD"
cp native-multipart.rs sdks/rust/accounts-sdk/src/lib.rs
cp "$SCRIPT_DIR/fixtures/rust_sdk_multipart.rs" sdks/rust/accounts-sdk/examples/native_multipart.rs
cargo clippy -p accounts-sdk --lib --example native_multipart -- -D warnings
cargo build -p accounts-sdk --example native_multipart
"$TSC_BIN" --strict --target ES2022 --module ESNext --moduleResolution Bundler --lib ES2022,DOM --outDir native-multipart-dist native-multipart.ts
node "$SCRIPT_DIR/rust_multipart_response_native.mjs" "$BINARY" "$(dirname "$BINARY")/examples/native_multipart" "$PWD/generated-wire-fixtures/typescript_sdk_server.mjs" "$PWD/native-multipart-dist/native-multipart.js"
