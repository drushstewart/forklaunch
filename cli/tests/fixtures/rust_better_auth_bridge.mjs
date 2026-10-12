import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
export function nativeAuthBridge(root, options = {}) {
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
      options.features ?? 'http',
      '--example',
      'http_parity'
    ],
    {
      cwd: root,
      env: { ...process.env, ...options.env, CARGO_INCREMENTAL: '0' },
      stdio: ['pipe', 'pipe', 'inherit']
    }
  );
  const pending = [];
  createInterface({ input: child.stdout }).on('line', (line) =>
    pending.shift().resolve(JSON.parse(line))
  );
  const completion = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`Native auth bridge exited ${code}`));
      for (const entry of pending.splice(0))
        entry.reject(new Error('Native auth bridge closed'));
    });
  });
  completion.catch(() => {});
  return {
    native: (input) =>
      new Promise((resolve, reject) => {
        pending.push({ resolve, reject });
        child.stdin.write(`${JSON.stringify(input)}\n`);
      }),
    close: async () => {
      child.stdin.end();
      await completion;
    }
  };
}
