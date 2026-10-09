import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createForkLaunchSdk } from './sdks/typescript/dist/index.js';

const processes = [];
const role = process.env.FORKLAUNCH_TEST_ROLE;
const databaseUrl = new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);
databaseUrl.username = role;
databaseUrl.password = 'parity-test-only';
async function start(tenant) {
  const child = spawn(process.argv[2], [], {
    env: { ...process.env, DATABASE_URL: databaseUrl.toString(), TENANT_ID: `${role}-${tenant}`, HOST: '127.0.0.1', SERVICE_PORT: '0' },
    stdio: ['ignore', 'pipe', 'pipe']
  });
  const exited = once(child, 'exit');
  processes.push({ child, exited });
  let stderr = '';
  child.stderr.on('data', chunk => stderr += chunk);
  const baseUrl = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`IAM startup timed out: ${stderr}`)), 15000);
    let output = '';
    child.stdout.on('data', chunk => {
      output += chunk;
      const match = output.match(/listening on (http:\/\/[^\s]+)/);
      if (match) { clearTimeout(timeout); resolve(match[1]); }
    });
    child.once('error', error => { clearTimeout(timeout); reject(error); });
    child.once('exit', code => { clearTimeout(timeout); reject(new Error(`IAM exited ${code}: ${stderr}`)); });
  });
  return { baseUrl, child, exited };
}

try {
  const first = await start('a');
  const second = await start('b');
  const publicSdk = createForkLaunchSdk({ baseUrl: first.baseUrl });
  const login = await publicSdk.iam.auth.authenticate({ body: { email: 'member@example.com', password: 'first-password' } });
  assert.equal(login.code, 200);
  const token = login.response.id;
  const sdk = createForkLaunchSdk({ baseUrl: first.baseUrl, headers: { authorization: `Bearer ${token}` } });
  const page = await sdk.iam.users.listUsers({ query: { limit: 1 } });
  assert.equal(page.code, 200);
  assert.equal(page.response.items[0].display_name, 'First');
  assert.equal(page.response.next_cursor, 'user-1');
  const next = await sdk.iam.users.listUsers({ query: { limit: 1, cursor: page.response.next_cursor } });
  assert.equal(next.response.items[0].id, 'user-2');
  assert.equal(next.response.next_cursor, null);
  const wrongTenant = await createForkLaunchSdk({ baseUrl: second.baseUrl, headers: { authorization: `Bearer ${token}` } }).iam.users.listUsers({query:{}});
  assert.equal(wrongTenant.code, 401);
  assert.equal((await publicSdk.iam.users.listUsers({query:{}})).code, 401);
  assert.equal((await fetch(`${first.baseUrl}/health`)).status, 200);
  // A process restart preserves sessions and records in PostgreSQL.
  first.child.kill('SIGTERM');
  assert.equal((await first.exited)[0], 0);
  const restarted = await start('a');
  const restartedSdk = createForkLaunchSdk({ baseUrl: restarted.baseUrl, headers: { authorization: `Bearer ${token}` } });
  assert.equal((await restartedSdk.iam.users.getUser({ params: { id: 'user-1' } })).response.email, 'member@example.com');
  assert.equal((await restartedSdk.iam.auth.logout()).code, 204);
  assert.equal((await restartedSdk.iam.users.listUsers({query:{}})).code, 401);
  console.log('PASS generated PostgreSQL IAM, typed TypeScript SDK, tenant isolation, restart persistence, revocation, and SIGTERM');
} finally {
  for (const { child } of processes) {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill('SIGTERM');
      setTimeout(() => child.kill('SIGKILL'), 10000).unref();
    }
  }
  await Promise.all(processes.map(({ exited }) => exited));
}
