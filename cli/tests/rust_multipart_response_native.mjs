import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
const {createForkLaunchSdk}=await import(pathToFileURL(process.argv[5]));
const children = [];
async function startServer(program, args = []) {
  const process = spawn(program, args, {
    env: { ...globalThis.process.env, HOST: '127.0.0.1', PORT: '0', RUST_LOG: 'info', MCP_ENABLED: 'true', PROTOCOL: 'http', VERSION: 'v1', DOCS_PATH: '/docs' },
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
async function stopServer(child) {
  child.process.kill('SIGTERM');
  const deadline = setTimeout(() => child.process.kill('SIGKILL'), 10000);
  try {
    const [code, signal] = await child.exited;
    assert.equal(code, 0, `shutdown failed: ${signal} ${child.stderr}`);
  } finally { clearTimeout(deadline); }
}
async function verifyMultipart(sdk, baseUrl) {
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

}

try {
  for(const [command,args] of [[process.argv[2],[]],[process.execPath,[process.argv[4]]]]) {
    const service=await startServer(command,args);
    await verifyMultipart(createForkLaunchSdk({baseUrl:service.baseUrl}),service.baseUrl);
    const client=spawn(process.argv[3],[service.baseUrl],{stdio:['ignore','pipe','pipe']});
    let output='';client.stdout.on('data',chunk=>output+=chunk);client.stderr.on('data',chunk=>output+=chunk);
    const deadline=setTimeout(()=>client.kill('SIGKILL'),15000);
    try {const [code]=await once(client,'exit');assert.equal(code,0,output);}
    finally {clearTimeout(deadline);if(client.exitCode===null&&client.signalCode===null)client.kill('SIGKILL');}
    await stopServer(service);
  }
  console.log('PASS both native multipart SDKs against Rust and TypeScript HTTP services');
} finally {
  for(const child of children) if(child.process.exitCode===null&&child.process.signalCode===null){child.process.kill('SIGKILL');await child.exited.catch(()=>{});}
}
