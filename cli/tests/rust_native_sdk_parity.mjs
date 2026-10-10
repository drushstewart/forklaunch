import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { appendFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const temporary = mkdtempSync(join(tmpdir(), 'forklaunch-native-sdk-parity-'));
const require = createRequire(import.meta.url);
function run(command, args, env = process.env) {
  const result = spawnSync(command, args, { cwd: temporary, env, encoding: 'utf8', timeout: 300000, maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, `${result.error || ''}\n${result.stdout}\n${result.stderr}`);
}
let server;
try {
  run('cargo', ['test', '--quiet', '--manifest-path', join(root, 'framework-rs/Cargo.toml'), '-p', 'forklaunch-sdk-rust', '-p', 'forklaunch-sdk-typescript', 'native_sdk_preserves'], { ...process.env, FORKLAUNCH_TEST_NATIVE_SDK_OUTPUT: temporary });
  writeFileSync(join(temporary, 'tsconfig.json'), JSON.stringify({ compilerOptions: {
    strict: true, target: 'ES2022', module: 'NodeNext', moduleResolution: 'NodeNext',
    lib: ['ES2022', 'DOM'], types: [], outDir: 'compiled'
  }, include: ['native-sdk.ts'] }));
  run(process.env.TSC_BIN || join(root, 'framework/node_modules/.bin/tsc'), ['--project', join(temporary, 'tsconfig.json')]);
  appendFileSync(join(temporary, 'native-sdk.rs'), String.raw`
fn main() {
    let client = ForkLaunchClient::new("http://localhost");
    let request = client.post_binary_request(PostBinaryRequest { body: Some(vec![0, 255, 1]), ..Default::default() });
    assert_eq!(request.body_bytes, Some(vec![0, 255, 1]));
    assert!(request.body.is_none());
    assert_eq!(request.body_content_type, Some("application/octet-stream"));
    let file = client.post_file_request(PostFileRequest { body: Some(vec![137, 80, 78, 71]), ..Default::default() });
    assert_eq!(file.body_content_type, Some("image/png"));
    assert_eq!(file.body_bytes, Some(vec![137, 80, 78, 71]));
    let text = client.post_text_request(PostTextRequest { body: Some("hello 水".into()), ..Default::default() });
    assert_eq!(text.body.as_deref(), Some("hello 水"));
    assert_eq!(text.body_content_type, Some("text/plain; charset=utf-8"));
}
`);
  run('rustc', ['--edition=2024', '-D', 'warnings', join(temporary, 'native-sdk.rs'), '-o', join(temporary, 'native-sdk')]);
  run(join(temporary, 'native-sdk'), []);
  server = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ method: request.method, body: Buffer.concat(chunks).toString('hex'), contentType: request.headers['content-type'] }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const { MediaClient } = require(join(temporary, 'compiled/native-sdk.js'));
  const client = new MediaClient(`http://127.0.0.1:${server.address().port}`);
  for (const [method, body, expected, contentType] of [
    ['postJson', { user: '水', active: true }, Buffer.from(JSON.stringify({ user: '水', active: true })), 'application/vnd.forklaunch+json'],
    ['postText', 'hello 水', Buffer.from('hello 水'), 'text/plain; charset=utf-8'],
    ['postBinary', new Uint8Array([99, 0, 255, 1, 99]).subarray(1, 4), Buffer.from([0, 255, 1]), 'application/octet-stream'],
    ['postFile', new Blob([new Uint8Array([137, 80, 78, 71])]), Buffer.from([137, 80, 78, 71]), 'image/png'],
    ['postUrlencoded', new URLSearchParams([['key', 'a b'], ['key', 'c/d']]), Buffer.from('key=a+b&key=c%2Fd'), 'application/x-www-form-urlencoded']
  ]) {
    const echoed = await (await client[method]({ body })).json();
    assert.equal(echoed.method, 'POST');
    assert.equal(echoed.body, expected.toString('hex'));
    assert.equal(echoed.contentType, contentType);
  }
  const form = new FormData();
  form.append('label', 'hello 水');
  form.append('file', new Blob([new Uint8Array([0, 255, 1])]), 'data.bin');
  const echoed = await (await client.postForm({ body: form })).json();
  assert.match(echoed.contentType, /^multipart\/form-data; boundary=.+/);
  const bytes = Buffer.from(echoed.body, 'hex');
  assert.ok(bytes.includes(Buffer.from('hello 水')));
  assert.ok(bytes.includes(Buffer.from([0, 255, 1])));
  assert.ok(bytes.includes(Buffer.from('filename="data.bin"')));
  console.log('PASS framework SDKs: strict TypeScript and Rust compilation, raw text/binary/file bytes, vendor JSON media, real multipart boundaries and URL-encoded uploads');
} finally {
  if (server) {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
  rmSync(temporary, { recursive: true, force: true });
}
