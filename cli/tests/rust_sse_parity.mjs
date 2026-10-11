import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile, realpath, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const require = createRequire(import.meta.url);
const tsc = process.env.TSC_BIN ?? path.join(root, 'framework/node_modules/.bin/tsc');
const ts = require(require.resolve('typescript', { paths: [path.dirname(await realpath(tsc))] }));
const temporary = await mkdtemp(path.join(tmpdir(), 'fl-sse-parity-'));
let chunks;
async function fetcher() {
  let index = 0;
  return new Response(new ReadableStream({ pull(controller) {
    if (index === chunks.length) controller.close();
    else controller.enqueue(Uint8Array.from(chunks[index++]));
  } }), { headers: { 'content-type': 'text/event-stream' } });
}
async function load(file, imports = {}) {
  const compiled = ts.transpileModule(await readFile(path.join(root, file), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports = {};
  vm.runInNewContext(compiled, { exports, require(spec) { if (spec in imports) return imports[spec]; throw Error(`unexpected import ${spec}`); }, fetch: fetcher, Buffer, URL, URLSearchParams, Response, FormData, Blob, File, TextDecoder, ReadableStream });
  return exports;
}
try {
  const template = (await readFile(path.join(root, 'cli/src/templates/sdk/typescript/index.ts'), 'utf8'))
    .replace('{{{form_stringify_source}}}', await readFile(path.join(root, 'framework/common/src/safeStringify.ts'), 'utf8'))
    .replace('{{{sse_source}}}', await readFile(path.join(root, 'cli/src/templates/sdk/typescript/sse.ts'), 'utf8'));
  const runtime = template.slice(0, template.indexOf('{{#operation_types}}')) + template.slice(template.indexOf('function pathWithParams'), template.indexOf('export function createForkLaunchSdk')) + '\nexport { request };\n';
  await writeFile(path.join(temporary, 'runtime.ts'), runtime);
  await writeFile(path.join(temporary, 'tsconfig.json'), JSON.stringify({ compilerOptions: { strict: true, target: 'ES2022', module: 'CommonJS', lib: ['ES2022', 'DOM'], types: [], outDir: 'compiled' }, include: ['runtime.ts'] }));
  execFileSync(tsc, ['--project', path.join(temporary, 'tsconfig.json')], { encoding: 'utf8', timeout: 120000 });
  const { request } = require(path.join(temporary, 'compiled/runtime.js'));
  const common = { ...(await load('framework/common/src/safeStringify.ts')), ...(await load('framework/common/src/safeParse.ts')), ...(await load('framework/common/src/openApiCompliantPath.ts')) };
  const { getSdkPath } = await load('framework/universal-sdk/src/core/resolvePath.ts', { './regex': { generateStringFromRegex() { throw Error('unexpected regex'); } } });
  const { UniversalSdk } = await load('framework/universal-sdk/src/universalSdk.ts', { '@forklaunch/common': common, ajv: { default: class {} }, 'ajv-formats': { default() {} }, './core/coerceSpecialTypes': { coerceSpecialTypes: value => value }, './core/mapContentType': { mapContentType: value => value }, './core/openApi': { refreshOpenApi() { throw Error('unexpected refresh'); } }, './core/resolvePath': { getSdkPath } });
  const universal = new UniversalSdk('http://127.0.0.1:9000', { validate: () => true }, {}, undefined);
  universal.registryOpenApiJson = { latest: { paths: { '/events': { get: {
    responses: { 200: { content: { 'text/event-stream': { schema: {} } } } }
  } } } } };
  // Common terminated frames supported by the actual TypeScript client. Additional
  // standards cases (multiline data, bare CR, whitespace and EOF) are native unit gates.
  const bodies = ['', ': heartbeat\n\n', 'data: {}\n\n', 'id: first\ndata: {"label":"水 🦀","active":false}\n\ndata: [0,null,"two"]\n\n', 'id: first\r\ndata: 42\r\n\r\nid:\r\ndata: null\r\n\r\n', '\uFEFFevent: update\nretry: 1000\nid: same\ndata: plain text\n\ndata:\n\n', 'unknown: ignored\n: keepalive\ndata: false\n\ndata: ""\n\n'];
  const cases = bodies.flatMap(body => {
    const bytes = Buffer.from(body);
    return [1,2,3,5,Math.max(1,bytes.length)].map(size => Array.from({ length: Math.ceil(bytes.length/size) }, (_, index) => [...bytes.subarray(index*size, (index+1)*size)]));
  });
  await writeFile(path.join(temporary, 'cases.json'), JSON.stringify(cases));
  execFileSync('cargo', ['test', '--quiet', '--manifest-path', 'framework-rs/Cargo.toml', '-p', 'forklaunch-http', 'native_sse_transport_conformance'], { cwd: root, env: { ...process.env, FORKLAUNCH_TEST_SSE_INPUT: path.join(temporary, 'cases.json'), FORKLAUNCH_TEST_SSE_OUTPUT: path.join(temporary, 'events.json') }, encoding: 'utf8', timeout: 180000 });
  const native = JSON.parse(await readFile(path.join(temporary, 'events.json'), 'utf8'));
  for (let index = 0; index < cases.length; index++) {
    chunks = cases[index];
    const response = await universal.execute('/events', 'get', 'latest', {});
    const events = [];
    for await (const event of response.response) events.push(event);
    assert.deepEqual(JSON.parse(JSON.stringify(events)), native[index], `actual TypeScript SSE case ${index}`);
    const generated = await request({ baseUrl: 'http://127.0.0.1:9000', fetch: fetcher }, { service: 'streams', router: 'events', operation: 'watchEvents', method: 'GET', path: '/events', responseKinds: { 200: 'sse' }, input: {} });
    const actual = [];
    for await (const event of generated.response) actual.push(event);
    assert.deepEqual(actual, native[index], `generated TypeScript SSE case ${index}`);
  }
  console.log(`PASS ${cases.length} actual TypeScript universal SDK/native codec/generated SDK SSE comparisons: chunked UTF-8, LF/CRLF, JSON/scalar/text data, IDs, resets, comments and empty streams`);
} finally { await rm(temporary, { recursive: true, force: true }); }
