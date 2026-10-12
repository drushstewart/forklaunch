// Execute the configured server-only API, including authoritative storage effects.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const reference = await betterAuthReference(root);
const RealDate = Date,
  start = 1800000000000;
async function atTime(now, work) {
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [now]));
    }
    static [Symbol.hasInstance](value) {
      return value instanceof RealDate;
    }
    static now() {
      return now;
    }
  };
  try {
    return await work();
  } finally {
    globalThis.Date = RealDate;
  }
}
const clients = [];
for (let i = 0; i < 7; i++) {
  const client = reference.client();
  await atTime(start, () =>
    client.call('/sign-up/email', {
      name: 'Fixture Person',
      firstName: 'Fixture',
      lastName: 'Person',
      email: `set-${i}@fixture.test`,
      password: 'test-only-password',
    }),
  );
  const user = reference.database.user.at(-1),
    account = reference.database.account.at(-1),
    session = reference.database.session.at(-1);
  clients.push({
    headers: new Headers({
      cookie: [...client.cookies]
        .map(([key, value]) => `${key}=${value}`)
        .join('; '),
    }),
    token: session.token,
    user,
    account,
  });
}
// Existing credential, absent credential, empty/null/missing password, mismatched
// legacy credential and a session that will expire before its call.
reference.database.account.splice(
  reference.database.account.indexOf(clients[1].account),
  1,
);
clients[2].account.password = '';
clients[3].account.password = null;
delete clients[4].account.password;
clients[5].account.accountId = 'legacy-subject';
clients[6].account.password = '';
const snapshot = () =>
  JSON.parse(
    JSON.stringify({
      users: reference.database.user,
      accounts: reference.database.account,
      sessions: reference.database.session,
    }),
  );
const inputs = [{ op: 'seed', now: start, ...snapshot() }],
  expected = [];
async function compare(client, password, now = start) {
  let result;
  try {
    result = {
      body: await atTime(now, () =>
        reference.auth.api.setPassword({
          body: { newPassword: password },
          headers: client?.headers ?? new Headers(),
        }),
      ),
    };
  } catch (error) {
    result = { error: { code: error.body.code, message: error.body.message } };
  }
  inputs.push({ op: 'set', now, token: client?.token ?? 'unknown', password });
  expected.push(result);
  inputs.push({ op: 'records', now });
  expected.push(snapshot());
}
await compare(null, 'short');
for (const password of ['', 'short', 'x'.repeat(129), '😀'.repeat(65)])
  await compare(clients[0], password);
await compare(clients[0], 'replacement-password');
for (const client of clients.slice(1, 6)) {
  await compare(client, 'first-set-password');
  await compare(client, 'second-set-password');
}
await compare(clients[6], 'expired-password', start + 86400001);
// Old but live sessions refresh before a subsequent password error.
await compare(clients[0], 'replacement-password', start + 3600000);
const run = spawnSync(
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
    'http_parity',
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    input: inputs.map((value) => JSON.stringify(value)).join('\n') + '\n',
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
  },
);
assert.equal(run.status, 0, run.stderr);
const actual = run.stdout
  .trim()
  .split('\n')
  .map((line) => JSON.parse(line))
  .slice(1);
assert.equal(actual.length, expected.length);
function normalize(value, hashes = new Map()) {
  if (Array.isArray(value)) return value.map((row) => normalize(row, hashes));
  if (value && typeof value === 'object')
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => {
        if (key === 'password' && typeof item === 'string' && item) {
          assert.match(item, /^[a-f0-9]{32}:[a-f0-9]{128}$/);
          if (!hashes.has(item)) hashes.set(item, `[hash:${hashes.size}]`);
          return [key, hashes.get(item)];
        }
        if (key === 'id' && value.providerId)
          return [
            key,
            `account:${value.userId}:${value.accountId}:${value.providerId}`,
          ];
        return [key, normalize(item, hashes)];
      }),
    );
  return value;
}
for (let i = 0; i < expected.length; i++)
  assert.deepEqual(
    normalize(actual[i]),
    normalize(expected[i]),
    `case ${i}: ${JSON.stringify(inputs[i + 1])}`,
  );
console.log(
  `Better Auth server-only setPassword: ${expected.length} actual API/storage comparisons passed`,
);
