import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
import assert from 'node:assert/strict';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { readFile } from 'node:fs/promises';
const [binary, sdkPath, consumerManifest, publicRuntime, openapiPath] = process.argv.slice(2);
const child = spawn(binary, [], { env: { ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('OTEL_'))), HOST:'127.0.0.1', PORT:'0',RUST_LOG:'info',MCP_ENABLED:'true',PROTOCOL:'http',VERSION:'v1',DOCS_PATH:'/docs', HMAC_SECRET_KEY:'method-parity-owned-test-secret', JWKS_PUBLIC_KEY_URL:'http://127.0.0.1:9/unused' }, stdio:['ignore','pipe','pipe'] });
let output = '', errors = '', closed = false;
child.stdout.on('data', data => { output += data; });
child.stderr.on('data', data => { errors += data; });
const exit = new Promise(resolve => child.once('close', code => { closed = true; resolve(code); }));
try {
  let base;
  for (let attempt=0;attempt<200;attempt++) {
    base = output.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)?.[1];
    if (base) break;
    assert.ok(!closed, errors);
    await new Promise(resolve => setTimeout(resolve,50));
  }
  assert.ok(base, errors);
  const tools=await probeMcp(base);
  const probes=tools.filter(tool=>tool.description==='HTTP method');assert.equal(probes.length,27);
  for(const tool of probes){const result=await mcpRpc(base,'tools/call',{name:tool.name,arguments:{}});assert.equal(result.isError,false,tool.name);assert.equal(result.content[0].text,'');}
  // Node/browser Fetch forbids TRACE. A custom HTTP transport can send it while
  // retaining the generated client's request and response types.
  const transport = (url, options) => new Promise((resolve,reject) => {
    const request = http.request(url, { method: options.method, headers: Object.fromEntries(new Headers(options.headers)) }, response => {
      const chunks=[];
      response.on('data',chunk => chunks.push(chunk));
      response.on('end',() => resolve(new Response(response.statusCode===204?null:Buffer.concat(chunks), { status: response.statusCode, headers: response.headers })));
    });
    request.on('error',reject);
    request.end(options.body);
  });
  if (sdkPath !== '-') {
    const sdk = await import(pathToFileURL(sdkPath));
    const client = sdk.createForkLaunchSdk({serviceBaseUrls:{cac:base},fetch:transport});
    assert.equal(Object.keys(client.cac.verbs).length,27);
    for (const [name,operation] of Object.entries(client.cac.verbs)) {
      const result = await operation();
      assert.equal(result.status,204,name);
      assert.equal(result.response,undefined,name);
    }
  }
  const document = JSON.parse(await readFile(openapiPath, 'utf8'));
  assert.equal(Object.values(document.paths).reduce((count, item) => count + Object.keys(item['x-forklaunch-http-methods'] ?? {}).length, 0), 19);
  const {universalSdk} = await import(pathToFileURL(publicRuntime));
  const publicClient = await universalSdk({host:base,registryOptions:{raw:document},fetch:transport});
  const operations = Object.entries(document.paths).filter(([path])=>path.startsWith('/verbs/')).flatMap(([,item])=>
    [...Object.entries(item).filter(([key])=>key!=='x-forklaunch-http-methods').map(([,value])=>value), ...Object.values(item['x-forklaunch-http-methods']??{})]
      .filter(value=>value && typeof value==='object' && value['x-forklaunch-operation'])
      .map(value=>value['x-forklaunch-operation']));
  assert.equal(operations.length,27);
  for (const operation of operations) {
    const call = operation.clientPath.reduce((value,key)=>value[key],publicClient);
    const result = await call({});
    assert.equal(result.status,204,operation.clientPath.join('.'));
    assert.equal(result.response,undefined,operation.clientPath.join('.'));
  }
  const test = spawn('cargo',['test','--manifest-path',consumerManifest,'--test','methods'], { env:{...process.env,METHODS_BASE_URL:base},stdio:'inherit' });
  assert.equal(await new Promise(resolve => test.once('close',resolve)),0);
} finally {
  if (!closed) child.kill('SIGTERM');
  assert.equal(await exit,0,errors);
}
console.log('PASS all 27 HTTP methods through public universalSdk, generated TypeScript/Rust SDKs and MCP');
