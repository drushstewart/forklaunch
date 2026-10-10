import assert from 'node:assert/strict';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
const [binary, sdkPath] = process.argv.slice(2);
const child = spawn(binary, [], { env: { ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('OTEL_'))), HOST:'127.0.0.1', SERVICE_PORT:'0', HMAC_SECRET_KEY:'method-parity-owned-test-secret', JWKS_PUBLIC_KEY_URL:'http://127.0.0.1:9/unused' }, stdio:['ignore','pipe','pipe'] });
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
  // Node/browser Fetch forbids TRACE. A custom HTTP transport can send it while
  // retaining the generated client's request and response types.
  const transport = (url, options) => new Promise((resolve,reject) => {
    const request = http.request(url, { method: options.method, headers: options.headers }, response => {
      const chunks=[];
      response.on('data',chunk => chunks.push(chunk));
      response.on('end',() => resolve(new Response(response.statusCode===204?null:Buffer.concat(chunks), { status: response.statusCode, headers: response.headers })));
    });
    request.on('error',reject);
    request.end(options.body);
  });
  const sdk = await import(pathToFileURL(sdkPath));
  const client = sdk.createForkLaunchSdk({serviceBaseUrls:{cac:base},fetch:transport});
  assert.equal(Object.keys(client.cac.verbs).length,27);
  for (const [name,operation] of Object.entries(client.cac.verbs)) {
    const result = await operation();
    assert.equal(result.status,204,name);
    assert.equal(result.response,undefined,name);
  }
  const test = spawn('cargo',['test','-p','cac-sdk','--test','methods'], { env:{...process.env,METHODS_BASE_URL:base},stdio:'inherit' });
  assert.equal(await new Promise(resolve => test.once('close',resolve)),0);
} finally {
  if (!closed) child.kill('SIGTERM');
  assert.equal(await exit,0,errors);
}
console.log('PASS real HTTP transport with all methods through generated TypeScript and Rust SDKs');
