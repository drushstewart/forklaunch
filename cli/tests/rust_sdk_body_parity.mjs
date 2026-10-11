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
const temporary = await mkdtemp(path.join(tmpdir(), 'fl-sdk-body-parity-'));
let captured;
async function fetcher(url, options) {
  captured = new Request(url, options);
  return new Response('{"ok":true}', { headers: { 'content-type': 'application/json' } });
}
async function load(file, imports = {}) {
  const compiled = ts.transpileModule(await readFile(path.join(root, file), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const exports = {};
  vm.runInNewContext(compiled, { exports, require(spec) { if (spec in imports) return imports[spec]; throw Error(`unexpected import ${spec}`); }, fetch: fetcher, Buffer, URL, URLSearchParams, Response, FormData, Blob, File, Map, Set, Date, RegExp, ArrayBuffer, Uint8Array, DataView, Error });
  return exports;
}
async function describe(request) {
  const contentType = request.headers.get('content-type');
  if (contentType.startsWith('multipart/')) {
    const form = await request.formData();
    return Promise.all([...form].map(async ([name, value]) => typeof value === 'string' ? [name, value] : [name, value.name, value.type, [...new Uint8Array(await value.arrayBuffer())]]));
  }
  return [contentType, await request.text()];
}
try {
  const stringifier = await readFile(path.join(root, 'framework/common/src/safeStringify.ts'), 'utf8');
  const template = (await readFile(path.join(root, 'cli/src/templates/sdk/typescript/index.ts'), 'utf8')).replace('{{{transport_source}}}', await readFile(path.join(root, 'cli/src/templates/sdk/typescript/transport.ts'), 'utf8')).replace('{{{form_stringify_source}}}', stringifier);
  const sseSource = await readFile(path.join(root, 'cli/src/templates/sdk/typescript/sse.ts'), 'utf8');
  const runtime = template.slice(0, template.indexOf('{{#operation_types}}')).replace('{{{sse_source}}}', sseSource) + template.slice(template.indexOf('function pathWithParams'), template.indexOf('export function createForkLaunchSdk')) + '\nexport { request };\n';
  await writeFile(path.join(temporary, 'runtime.ts'), runtime);
  await writeFile(path.join(temporary, 'tsconfig.json'), JSON.stringify({ compilerOptions: { strict: true, target: 'ES2022', module: 'CommonJS', lib: ['ES2022', 'DOM'], types: [], outDir: 'compiled' }, include: ['runtime.ts'] }));
  execFileSync(tsc, ['--project', path.join(temporary, 'tsconfig.json')], { encoding: 'utf8', timeout: 120000 });
  const { request } = require(path.join(temporary, 'compiled/runtime.js'));
  const { safeStringify } = await load('framework/common/src/safeStringify.ts');
  const { openApiCompliantPath } = await load('framework/common/src/openApiCompliantPath.ts');
  const { getSdkPath } = await load('framework/universal-sdk/src/core/resolvePath.ts', { './regex': { generateStringFromRegex() { throw Error('unexpected regex'); } } });
  const { UniversalSdk } = await load('framework/universal-sdk/src/universalSdk.ts', { '@forklaunch/common': { safeStringify, openApiCompliantPath, safeParse: JSON.parse }, ajv: { default: class {} }, 'ajv-formats': { default() {} }, './core/coerceSpecialTypes': { coerceSpecialTypes: value => value }, './core/mapContentType': { mapContentType: value => value }, './core/openApi': { refreshOpenApi() { throw Error('unexpected refresh'); } }, './core/resolvePath': { getSdkPath } });
  const universal = new UniversalSdk('http://127.0.0.1:9000', { validate: () => true }, {}, undefined);
  universal.registryOpenApiJson = {
    latest: { paths: { '/upload': { post: {
      responses: { 200: { content: { 'application/json': { schema: {} } } } }
    } } } }
  };
  const forms = [{}, { label: '水 & +', empty: '', flag: false, zero: 0 }, { values: ['a,b', '水', ''], empty: [] }, { nested: { active: true, ids: ['x', 'y'] }, nullable: null, absent: undefined }, { line: 'a\nb\rc\r\nd', 'quoted"\r\nname': 'safe' }, { number: NaN, infinity: Infinity, bigint: 123n, collection: new Set(['a', 'b']), bytes: new Uint8Array([0,255]) }];
  let cases = 0;
  for (const kind of ['urlEncoded', 'multipart']) {
    for (const body of [...forms, ...(kind === 'multipart' ? [{ file: new File([new Uint8Array([0,255,1])], 'data.bin'), repeated: [new Blob([]), new File(['水'], 'unicode.txt', { type: 'text/plain' })] }] : [])]) {
      const typedBody = kind === 'multipart' ? { multipartForm: body } : { urlEncodedForm: body };
      await universal.execute('/upload', 'post', 'latest', { body: typedBody });
      const expected = await describe(captured);
      await request({ baseUrl: 'http://127.0.0.1:9000', fetch: fetcher, headers: { 'CONTENT-TYPE': 'incorrect' }, beforeRequest: meta => ({ ...meta, input: { ...meta.input, headers: { 'Content-Type': 'incorrect' } } }) }, { service: 'uploads', router: 'upload', operation: 'upload', method: 'POST', path: '/upload', requestKind: kind, requestContentType: kind === 'multipart' ? 'multipart/form-data' : 'application/x-www-form-urlencoded', input: { body } });
      assert.deepEqual(await describe(captured), expected, `${kind} actual TS universal SDK body`);
      cases++;
    }
  }
  for (const invalid of [null, [], 'text', 42]) {
    await assert.rejects(request({ baseUrl: 'http://127.0.0.1:9000', fetch: fetcher }, { service: 'uploads', router: 'upload', operation: 'upload', method: 'POST', path: '/upload', requestKind: 'multipart', input: { body: invalid } }), TypeError);
  }
  console.log(`PASS ${cases} actual TypeScript universal SDK/form comparisons: multipart files/repeats/Unicode/empty forms, URL-encoded JSON composites, special values and encoder-owned headers`);
} finally { await rm(temporary, { recursive: true, force: true }); }
