// Generated contract build hook. Source compiler belongs to the pinned framework.
import { createHash } from 'node:crypto';
import { readFile, readdir } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const name = process.argv[2];
if (!name || !/^[A-Za-z0-9_-]+$/.test(name)) throw new Error('Expected service name');
const configFile = path.join(root, '.forklaunch/typescript-build', `${name}.json`);
const config = JSON.parse(await readFile(configFile, 'utf8'));
const service = path.join(root, config.service);
function run(executable, args, cwd) {
  const result = spawnSync(executable, args, { cwd, env: process.env, stdio: 'inherit' });
  if (result.error || result.status !== 0) throw new Error(`Contract build command failed: ${executable}`);
}
let compiler = process.env.FORKLAUNCH_CONTRACT_COMPILER;
if (!compiler) {
  const target = process.env.CARGO_TARGET_DIR || path.join(root, '.forklaunch/tools/target');
  run('cargo', ['build', '--locked', '--manifest-path', path.join(root, config.compiler, 'Cargo.toml'), '--target-dir', target, '-p', 'forklaunch-core', '--features', 'typescript-source', '--example', 'typescript_contract_build'], root);
  compiler = path.join(path.resolve(root, target), 'debug/examples/typescript_contract_build');
}
async function fingerprint(directory, hash = createHash('sha256')) {
  for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a,b) => a.name.localeCompare(b.name))) {
    const file = path.join(directory, entry.name);
    if (entry.isSymbolicLink()) continue;
    if (entry.isDirectory()) {
      if (!['node_modules', 'dist', 'lib', 'target', 'production', 'client-sdk', '.git', '.forklaunch'].includes(entry.name)) await fingerprint(file, hash);
    } else if (entry.isFile() && (entry.name.endsWith('.ts') || entry.name === 'package.json' || entry.name.startsWith('tsconfig'))) {
      hash.update(file); hash.update(await readFile(file));
    }
  }
  return hash;
}
const watch = process.argv.includes('--watch');
let previous;
do {
  const current = (await fingerprint(path.join(root, config.modules))).digest('hex');
  if (current !== previous) {
    previous = current;
    try {
      run(config.runtime === 'bun' ? 'bun' : 'pnpm', config.runtime === 'bun' ? ['x', '--bun', 'tsc', '-b'] : ['exec', 'tsc', '-b'], service);
      run(compiler, ['--project', root, configFile], root);
    } catch (error) {
      if (!watch) throw error;
      console.error('Contract build failed; last-good SDKs retained:', error.message);
    }
  }
  if (watch) await new Promise(resolve => setTimeout(resolve, 300));
} while (watch);
