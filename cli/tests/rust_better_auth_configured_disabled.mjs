// Disabled configured features preserve actual schema/session/Origin ordering.
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
const owner = reference.client();
await atTime(start, () =>
  owner.call('/sign-up/email', {
    name: 'First Last',
    firstName: 'First',
    lastName: 'Last',
    email: 'disabled@fixture.test',
    password: 'test-only-password',
  }),
);
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
for (const [route, valid] of [
  ['/request-password-reset', { email: 'profile@fixture.test' }],
  ['/send-verification-email', { email: 'profile@fixture.test' }],
  ['/change-email', { newEmail: 'new@fixture.test' }],
  ['/delete-user', {}],
]) {
  for (const body of [
    undefined,
    null,
    [],
    {},
    valid,
    { ...valid, callbackURL: 'https://evil.test' },
    { ...valid, redirectTo: 'https://evil.test' },
  ]) {
    await compare(route, body, signed, start, 'POST');
    await compare(
      route,
      body,
      { 'content-type': 'application/json' },
      start,
      'POST',
    );
  }
}
await compare('/delete-user', { password: '', token: '', callbackURL: '' });
await compare('/change-email', { newEmail: 'invalid', callbackURL: 123 });
await compare('/request-password-reset', { email: 'invalid', redirectTo: 123 });
await compare('/send-verification-email', {
  email: 'invalid',
  callbackURL: 123,
});
await compare('/delete-user', { password: 123, token: true });
for (const [route, body] of [
  ['/request-password-reset', { email: 'invalid' }],
  ['/send-verification-email', { email: 'invalid' }],
  ['/change-email', { newEmail: 'invalid' }],
])
  await compare(route, body);

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
  `Better Auth configured disabled HTTP: ${expected.length} actual response/storage comparisons passed`,
);
