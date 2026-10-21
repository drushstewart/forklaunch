// Generated contract build hook. Compiler uses the selected framework dependency.
// Usage: contract-build.mjs <service> [--watch]; reads .forklaunch/build.json#contracts.<service>.
import { createHash } from 'node:crypto';
import { rmSync } from 'node:fs';
import { mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const name = process.argv[2];
if (!name || !/^[A-Za-z0-9_-]+$/.test(name)) throw new Error('Expected service name');
const planFile = path.join(root, '.forklaunch/build.json');
const config = JSON.parse(await readFile(planFile, 'utf8')).contracts?.[name];
if (!config) throw new Error(`Missing contract plan ${name} in ${planFile}`);
const service = path.join(root, config.service);
function run(executable, args, cwd) {
  const result = spawnSync(executable, args, { cwd, env: process.env, stdio: 'inherit' });
  if (result.error || result.status !== 0) throw new Error(`Contract build command failed: ${executable}`);
}
// The compiler builds into the workspace's normal Cargo target directory
// (honouring CARGO_TARGET_DIR and Cargo configuration), as reported by Cargo.
function targetDirectory(manifest) {
  if (process.env.CARGO_TARGET_DIR) return path.resolve(root, process.env.CARGO_TARGET_DIR);
  const result = spawnSync('cargo', ['metadata', '--format-version', '1', '--no-deps', '--manifest-path', manifest], { cwd: root, env: process.env, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error('Unable to locate the Cargo target directory');
  return JSON.parse(result.stdout).target_directory;
}
let compiler = process.env.FORKLAUNCH_CONTRACT_COMPILER;
if (!compiler) {
  if (config.compiler === '.forklaunch/tools/contract-compiler') {
    // Adding a contract service can legitimately change an existing root lock.
    // Cargo reuses unchanged resolutions and records new members normally;
    // production image compilation enforces --locked separately.
    const manifest = path.join(root, 'Cargo.toml');
    run('cargo', ['build', '--manifest-path', manifest, '-p', 'forklaunch-contract-compiler', '--bin', 'forklaunch-contract-compiler'], root);
    compiler = path.join(targetDirectory(manifest), 'debug/forklaunch-contract-compiler');
  } else {
    if (!['vendor/framework', '.forklaunch/framework-rs'].includes(config.compiler)) throw new Error('Unsupported generated contract compiler');
    const manifest = path.join(root, config.compiler, 'Cargo.toml');
    run('cargo', ['build', '--locked', '--manifest-path', manifest, '-p', 'forklaunch-core', '--features', 'typescript-source', '--example', 'typescript_contract_build'], root);
    compiler = path.join(targetDirectory(manifest), 'debug/examples/typescript_contract_build');
  }
}
// The compiler takes one plan per file; hand it this service's plan.
const scratch = await mkdtemp(path.join(os.tmpdir(), 'forklaunch-contract-'));
const configFile = path.join(scratch, `${name}.json`);
await writeFile(configFile, JSON.stringify(config));
process.on('exit', () => rmSync(scratch, { recursive: true, force: true }));
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => process.exit(130));
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
