// Configured profile/account handlers, including scoped storage and private fields.
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
const RealDate = Date;
const start = 1800000000000;
async function atTime(now, work) {
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [now]));
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
const owner = reference.client(),
  foreign = reference.client();
for (const [who, email] of [
  [owner, 'profile@fixture.test'],
  [foreign, 'foreign@fixture.test'],
])
  await atTime(start, () =>
    who.call('/sign-up/email', {
      name: 'First Last',
      firstName: 'First',
      lastName: 'Last',
      email,
      password: 'test-only-password',
      rememberMe: who === owner,
    }),
  );
const ownerId = reference.database.user.find(
  (row) => row.email === 'profile@fixture.test',
).id;
const account = reference.database.account.find(
  (row) => row.userId === ownerId,
);
reference.database.account.push({
  ...account,
  id: 'linked-profile-fixture',
  providerId: 'google',
  accountId: 'provider-subject',
  password: 'never-public',
  accessToken: 'never-public',
  refreshToken: 'never-public',
  idToken: 'never-public',
  privateField: 'never-public',
  scope:
    'openid, email, ,profile, email,\uFEFFtrimmed\uFEFF,\u0085preserved\u0085',
});
const snapshot = () =>
  JSON.parse(
    JSON.stringify({
      users: reference.database.user,
      accounts: reference.database.account,
      sessions: reference.database.session,
    }),
  );
const attributes = (cookies) =>
  cookies
    .map((cookie) => [cookie.split('=')[0], cookie.slice(cookie.indexOf(';'))])
    .sort();
const inputs = [{ op: 'seed', now: start, ...snapshot() }],
  expected = [];
let tick = 0;
const signed = {
  cookie: [...owner.cookies]
    .map(([name, value]) => `${name}=${value}`)
    .join('; '),
  origin: 'http://localhost:3000',
  'content-type': 'application/json',
};
async function compare(
  route,
  body,
  headers = signed,
  now = start + ++tick * 1000,
  method = body === undefined ? 'GET' : 'POST',
) {
  const response = await atTime(now, () =>
    reference.auth.handler(
      new Request(`http://localhost:3000/api/auth${route}`, {
        method,
        headers,
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      }),
    ),
  );
  let value = await response.text();
  try {
    value = JSON.parse(value);
  } catch {}
  inputs.push({
    op: 'request',
    now,
    path: route,
    method,
    headers: Object.fromEntries(
      new Request('http://localhost:3000', {
        method,
        headers,
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      }).headers,
    ),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  expected.push({
    route,
    status: response.status,
    body: value,
    cookies: attributes(response.headers.getSetCookie()),
  });
  inputs.push({ op: 'records', now });
  expected.push({ records: snapshot() });
}
await compare('/list-accounts', undefined, {});
await compare('/list-accounts');
const foreignHeaders = {
  ...signed,
  cookie: [...foreign.cookies]
    .map(([name, value]) => `${name}=${value}`)
    .join('; '),
};
await compare('/list-accounts', undefined, foreignHeaders);
await compare('/update-user', { name: 'Transient update' }, foreignHeaders);
await compare('/unlink-account', {
  accountId: reference.database.account.find((row) => row.userId !== ownerId)
    .id,
});
for (const body of [
  {},
  { email: 'new@fixture.test', name: 'ignored' },
  { privateSecret: 'ignored', id: 'injected', emailVerified: true },
  { name: 'Changed Name' },
  { image: null },
  { firstName: null, lastName: 123, phoneNumber: ['opaque'] },
  { name: 123, image: false },
  { email: '', firstName: 'Updated' },
])
  await compare('/update-user', body);
await compare('/update-user', {}, {}, start, 'POST');
for (const body of [undefined, null, [], true])
  await compare('/update-user', body, signed, start, 'POST');
await compare('/unlink-account', {});
await compare('/unlink-account', { accountId: 'foreign-account' });
await compare('/unlink-account', { accountId: 'linked-profile-fixture' });
await compare('/list-accounts');
await compare('/unlink-account', { accountId: 'missing' });
await compare('/unlink-account', { accountId: account.id });
await compare(
  '/unlink-account',
  { accountId: account.id },
  signed,
  start + 86400000,
);
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
    input: inputs.map((input) => JSON.stringify(input)).join('\n') + '\n',
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
const mismatches = [];
for (let i = 0; i < expected.length; i++) {
  const want = expected[i];
  const observed = want.records
    ? { records: actual[i] }
    : {
        route: want.route,
        status: actual[i].status,
        body: actual[i].body,
        cookies: attributes(actual[i].headers['set-cookie'] ?? []),
      };
  try {
    assert.deepEqual(observed, want);
  } catch {
    mismatches.push({ actual: observed, expected: want });
  }
}
assert.deepEqual(mismatches, []);
console.log(
  `Better Auth profile/account HTTP: ${expected.length} actual response/storage comparisons passed`,
);
