// pnpm "deploy" here only assembles a disposable local production directory.
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm, lstat, readdir } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const root = await mkdtemp(path.join(os.tmpdir(), 'forklaunch-production-deps-'));
async function put(file, content) {
  await mkdir(path.dirname(path.join(root, file)), { recursive: true });
  await writeFile(path.join(root, file), typeof content === 'string' ? content : JSON.stringify(content));
}
function run(command, args) {
  return execFileSync(command, args, { cwd: path.join(root, 'source'), encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}
try {
  await put('source/package.json', { name: 'fixture', private: true, packageManager: 'pnpm@11.1.0', devDependencies: { typescript: '5.9.3' } });
  await put('source/pnpm-workspace.yaml', 'packages: [api, core]\ninjectWorkspacePackages: true\nsyncInjectedDepsAfterScripts: [build]\n');
  await put('source/core/package.json', { name: '@fixture/core', version: '0.0.0', type: 'module', main: 'index.js' });
  await put('source/core/index.js', 'export const answer = 42;\n');
  await put('source/api/package.json', { name: '@fixture/api', version: '0.0.0', type: 'module', dependencies: { '@fixture/core': 'workspace:*', 'is-number': '7.0.0' }, devDependencies: { typescript: '5.9.3' }, dependenciesMeta: { '@fixture/core': { injected: false } } });
  await put('source/api/server.js', "import { answer } from '@fixture/core'; import isNumber from 'is-number'; console.log(JSON.stringify({answer, valid:isNumber(answer)}));\n");
  run('pnpm', ['install', '--lockfile-only', '--ignore-scripts']);
  const locked = await readFile(path.join(root, 'source/pnpm-lock.yaml'), 'utf8');
  run('pnpm', ['install', '--frozen-lockfile', '--ignore-scripts']);
  await put('source/core/new.js', 'export const answer = 43;\n');
  await (await import('node:fs/promises')).rename(path.join(root, 'source/core/new.js'), path.join(root, 'source/core/index.js'));
  assert.equal(JSON.parse(run('node', ['api/server.js'])).answer, 43);
  run('pnpm', ['--filter', '@fixture/api', '--prod', 'deploy', '--offline', path.join(root, 'runtime')]);
  assert.equal(await readFile(path.join(root, 'source/pnpm-lock.yaml'), 'utf8'), locked);
  await assert.rejects(lstat(path.join(root, 'runtime/node_modules/typescript')), { code: 'ENOENT' });
  assert(!(await readdir(path.join(root, 'runtime/node_modules/.pnpm'))).some(name => name.startsWith('typescript@')));
  await rm(path.join(root, 'source'), { recursive: true });
  const result = execFileSync('node', [path.join(root, 'runtime/server.js')], { encoding: 'utf8' });
  assert.deepEqual(JSON.parse(result), { answer: 43, valid: true });
  console.log('PASS frozen workspace lock, atomic workspace SDK updates, isolated production dependency closure, executable local workspace links, no TypeScript compiler');
} finally {
  await rm(root, { recursive: true, force: true });
}
