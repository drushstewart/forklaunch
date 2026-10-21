import assert from 'node:assert/strict';
import {probeMcp,mcpRpc} from '../rust_mcp_probe.mjs';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
const { createForkLaunchSdk, forkLaunchQueryOptions, forkLaunchMutationOptions, ForkLaunchSdkHttpError } = await import(pathToFileURL(process.argv[5]));
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
    const bounded=await createForkLaunchSdk({baseUrl,fetch:fetcher,maxMultipartResponseBytes:16});
    await assert.rejects(()=>bounded.accounts.multipartResponse.getMultipartResponse({params:{mode:'valid'}}),/exceeds byte limit/);
    const invalid=await createForkLaunchSdk({baseUrl,fetch:fetcher,maxMultipartResponseBytes:0});
    await assert.rejects(()=>invalid.accounts.multipartResponse.getMultipartResponse({params:{mode:'valid'}}),/byte limit/);
    const adapted=await createForkLaunchSdk({baseUrl,fetch:fetcher});
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

  // A first key of `$serde_json::private::RawValue` is upstream serde_json's
  // raw-value marker, not object data; MCP arguments parse with stock serde_json.
  for(const key of ['$serde_json::private::Number']) {
    const body={value:{[key]:'123',nested:[{[key]:{literal:true}}]}};
    const result=await mcpRpc(service.baseUrl,'tools/call',{name:'echoJson',arguments:{body}});
    assert.deepEqual(result.structuredContent,body);
  }
  const baseUrl = service.baseUrl;
  const sdk = await createForkLaunchSdk({ baseUrl });
  const health = await sdk.accounts.health.getHealth();
  assert.equal(health.code, 200);
  assert.equal(health.response.status, 'ok');
  const users = await sdk.accounts.users.listUsers({query:{}});
  assert.equal(users.code, 200);
  assert.equal(users.response.records[0].id, 'welcome');
  const docs = await fetch(`${baseUrl}/docs`);
  assert.equal(docs.headers.get('content-type'), 'text/html; charset=utf-8');
  const documentation = await docs.text();
  assert(documentation.includes('/openapi.json'));
  assert(documentation.includes('api-reference'));
  const specification = await fetch(`${baseUrl}/openapi.json`);
  assert.equal(specification.headers.get('content-type'), 'application/json');
  assert.equal((await specification.json()).paths['/users'].get.parameters.find(field => field.name === 'label').in, 'query');
  assert.equal((await fetch(`${baseUrl}/legacy-docs`)).headers.get('content-type'), 'text/html; charset=utf-8');
  assert.equal((await fetch(`${baseUrl}/missing`)).status, 404);
  assert.equal((await fetch(`${baseUrl}/missing/users`)).status, 404);
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
  assert.equal((await fetch(`${typescript.baseUrl}/missing/users`)).status, 404);
  await verifyRustClient(typescript.baseUrl);
  const tsSdk = await createForkLaunchSdk({ baseUrl: typescript.baseUrl });
  const tsHealth = await tsSdk.accounts.health.getHealth();
  assert.deepEqual(tsHealth.response, health.response);
  assert.deepEqual((await tsSdk.accounts.users.listUsers({query:{}})).response, users.response);
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
