// Validate emitted packages without inherited node_modules or an injected compiler.
import {mkdtemp, readdir, copyFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
if (process.argv.length < 3) throw Error('Pass generated event package directories');
for (const source of process.argv.slice(2).map(value => resolve(value))) {
  const destination = await mkdtemp(join(tmpdir(), 'sdk-clean-install-'));
  try {
    for (const name of await readdir(source)) {
      if (['package.json', 'tsconfig.json', 'events.ts'].includes(name) || name.endsWith('.mjs') || name.endsWith('.d.mts')) {
        await copyFile(join(source, name), join(destination, name));
      }
    }
    execFileSync('pnpm', ['install', '--ignore-scripts', '--no-frozen-lockfile'], {cwd:destination, stdio:'inherit'});
    execFileSync('pnpm', ['build'], {cwd:destination, stdio:'inherit'});
    console.log(`PASS clean generated package install/build: ${source}`);
  } finally {
    await rm(destination, {recursive:true, force:true});
  }
}
