// Actual client/plugin -> native production handlers through the owned JSON-line bridge.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { exerciseTwoFactorClient } from './fixtures/rust_better_auth_two_factor_client.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const child = spawn(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    'blueprint-rs/Cargo.toml',
    '-p',
    'forklaunch-iam-better-auth',
    '--features',
    'http',
    '--example',
    'http_parity'
  ],
  {
    cwd: root,
    env: {
      ...process.env,
      CARGO_INCREMENTAL: '0',
      FORKLAUNCH_AUTH_TWO_FACTOR: 'true',
      NODE_ENV: 'test'
    },
    stdio: ['pipe', 'pipe', 'inherit']
  }
);
const pending = [];
const lines = createInterface({ input: child.stdout });
lines.on('line', (line) => pending.shift().resolve(JSON.parse(line)));
const completion = new Promise((resolve, reject) => {
  child.once('error', reject);
  child.once('exit', (code) => {
    if (code === 0) resolve();
    else reject(new Error(`Native TOTP bridge exited ${code}`));
    for (const call of pending.splice(0))
      call.reject(new Error('Native bridge closed'));
  });
});
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(JSON.stringify(input) + '\n');
  });

const now = 1800000000000;
try {
  await exerciseTwoFactorClient({
    baseURL: 'http://localhost:3000/api/auth',
    origin: 'http://localhost:3000',
    clock: () => now,
    label: 'actual twoFactorClient/native HTTP',
    fetchImpl: async (url, options = {}) => {
      const result = await native({
        op: 'request',
        now,
        path: new URL(url).pathname.replace(/^\/api\/auth/, ''),
        method: options.method ?? 'GET',
        headers: Object.fromEntries(new Headers(options.headers)),
        body: typeof options.body === 'string' ? options.body : ''
      });
      const headers = new Headers();
      for (const [name, values] of Object.entries(result.headers))
        for (const value of values) headers.append(name, value);
      return new Response(JSON.stringify(result.body), {
        status: result.status,
        headers
      });
    }
  });
} finally {
  child.stdin.end();
  await completion;
  lines.close();
}
