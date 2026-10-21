import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const root = await mkdtemp(path.join(os.tmpdir(), 'forklaunch-ts-production-'));
const helper = fileURLToPath(new URL('../src/init/rust_project/typescript_image/production_build.mjs', import.meta.url));
async function put(file, value) {
  await mkdir(path.dirname(path.join(root, file)), { recursive: true });
  await writeFile(path.join(root, file), typeof value === 'string' ? value : JSON.stringify(value));
}
function run(program, args) {
  return execFileSync(program, args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}
try {
  await put('package.json', { private: true, type: 'module' });
  run('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund', '--save-dev', 'esbuild@0.25.12', 'typescript@5.9.3']);
  await put('core/package.json', { name: '@fixture/core', type: 'module', main: 'lib/index.js' });
  await put('core/tsconfig.json', { compilerOptions: { target: 'ES2022', module: 'ESNext', moduleResolution: 'bundler', outDir: 'lib', sourceMap: true, inlineSources: true }, include: ['*.ts'] });
  await put('core/index.ts', "export { answer } from './answer';\n");
  await put('core/answer.ts', 'export const answer: number = 42;\n');
  await put('sdk/package.json', { name: '@fixture/sdk', type: 'module', exports: { '.': './index.ts' } });
  await put('sdk/.forklaunch-sdk-package.json', { format: 'forklaunch.sdk.package.v1' });
  await put('sdk/index.ts', 'export function sdkValue(value: number): number { return value; }\n');
  await put('api/package.json', { name: '@fixture/api', type: 'module' });
  await put('api/tsconfig.json', { compilerOptions: { target: 'ES2022', module: 'ESNext', moduleResolution: 'bundler', outDir: 'dist', sourceMap: true, inlineSources: true }, include: ['*.ts', 'migrations-postgresql/*.ts'] });
  await put('api/server.ts', "// @ts-ignore fixture workspace resolution is tested in the production bundler\nimport { answer } from '@fixture/core';\n// @ts-ignore generated SDK package resolves at bundle time\nimport { sdkValue } from '@fixture/sdk';\nconsole.log(JSON.stringify({answer:sdkValue(answer)}));\n");
  await put('api/mikro-orm.config.ts', "export default { migrations: { tableName: 'service_owned_migrations' } };\n");
  await put('api/migrations-postgresql/Migration001.ts', 'export class Migration001 { marker = "compiled migration"; }\n');
  run('node', ['node_modules/typescript/bin/tsc', '-p', 'core']);
  run('node', ['node_modules/typescript/bin/tsc', '-p', 'api']);
  await put('node_modules/@mikro-orm/core/package.json', { name: '@mikro-orm/core', type: 'module', exports: './index.js' });
  await put('node_modules/@mikro-orm/core/index.js', `export const MikroORM = { async init(config) {
    if (config.migrations.tableName !== 'service_owned_migrations') throw Error('lost migration config');
    return { getMigrator() { return { async up() { console.log(new config.migrations.migrationsList[0].class().marker); } }; }, async close(force) { if (!force) throw Error('pool not closed'); } };
  } };\n`);
  await put('build.json', { workspace: '.', packages: ['core', 'api', 'sdk'], output: 'runtime', processes: [{ package: '@fixture/api', directory: 'api', entries: ['server'], database: 'postgresql' }] });
  run('node', [helper, 'build.json']);
  // Prove runtime needs neither original TS sources nor compiler/loader packages.
  await rm(path.join(root, 'core'), { recursive: true });
  await rm(path.join(root, 'api'), { recursive: true });
  await rm(path.join(root, 'sdk'), { recursive: true });
  await rm(path.join(root, 'node_modules/typescript'), { recursive: true });
  await rm(path.join(root, 'node_modules/esbuild'), { recursive: true });
  assert.deepEqual(JSON.parse(run('node', ['runtime/api/server.mjs'])), { answer: 42 });
  assert.equal(run('node', ['runtime/api/migrate.mjs']).trim(), 'compiled migration');
  const map = JSON.parse(await readFile(path.join(root, 'runtime/api/server.mjs.map'), 'utf8'));
  assert(map.sources.some(source => source.endsWith('answer.ts')));
  assert(map.sourcesContent.some(source => source?.includes('answer: number = 42')));
  console.log('PASS compiled workspace imports, JS-only runtime, service-owned migration metadata and original TypeScript source maps');
} finally {
  await rm(root, { recursive: true, force: true });
}
