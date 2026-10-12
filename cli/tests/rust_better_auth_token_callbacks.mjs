// Actual GET token callback validation, redirects and verification cleanup.
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
const owner = reference.client(),
  social = reference.client();
for (const [who, email] of [
  [owner, 'reset@fixture.test'],
  [social, 'social-reset@fixture.test'],
])
  await atTime(start, () =>
    who.call('/sign-up/email', {
      name: 'First Last',
      firstName: 'First',
      lastName: 'Last',
      email,
      password: 'test-only-password',
    }),
  );
const ownerId = reference.database.user.find(
  (row) => row.email === 'reset@fixture.test',
).id;
const socialId = reference.database.user.find(
  (row) => row.email === 'social-reset@fixture.test',
).id;
const socialAccount = reference.database.account.find(
  (row) => row.userId === socialId,
);
socialAccount.providerId = 'google';
socialAccount.accountId = 'social-subject';
delete socialAccount.password;
for (const [id, token, user, created, expires] of [
  ['valid', 'valid', ownerId, start, start + 60000],
  ['expired', 'expired', ownerId, start, start - 1],
  ['missing-user', 'missing-user', 'absent', start, start + 60000],
  ['older-duplicate', 'duplicates', 'absent', start - 1, start + 60000],
  ['newer-duplicate', 'duplicates', ownerId, start, start + 60000],
  ['boundary', 'boundary', ownerId, start, start],
  ['social', 'social', socialId, start, start + 60000],
  ['encoded', 'encoded/token', ownerId, start, start + 60000],
  ['literal', 'literal%2Ftoken', ownerId, start, start + 60000],
])
  reference.database.verification.push({
    id,
    identifier: `reset-password:${token}`,
    value: user,
    createdAt: new RealDate(created),
    updatedAt: new RealDate(created),
    expiresAt: new RealDate(expires),
  });
const snapshot = () =>
  JSON.parse(
    JSON.stringify({
      users: reference.database.user,
      accounts: reference.database.account,
      sessions: reference.database.session,
      verifications: reference.database.verification,
    }),
  );
const inputs = [{ op: 'seed', now: start, ...snapshot() }],
  expected = [];
const headers = { 'content-type': 'application/json' };
async function compare(route, body, requestHeaders = headers) {
  const request = new Request(`http://localhost:3000/api/auth${route}`, {
    method: 'GET',
    headers: requestHeaders,
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const response = await atTime(start, () => reference.auth.handler(request));
  let value = await response.text();
  try {
    value = JSON.parse(value);
  } catch {}
  inputs.push({
    op: 'request',
    now: start,
    path: route,
    method: 'GET',
    headers: Object.fromEntries(request.headers),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  expected.push({
    route,
    status: response.status,
    body: value,
    location: response.headers.get('location'),
    contentType: response.headers.get('content-type'),
  });
  inputs.push({ op: 'records', now: start, withVerifications: true });
  expected.push({ records: snapshot() });
}
for (const route of [
  '/reset-password/valid',
  '/reset-password/valid?callbackURL=a&callbackURL=b',
  '/reset-password/valid?callbackURL=',
  '/reset-password/valid?callbackURL=https%3A%2F%2Fevil.test',
  '/reset-password/valid?callbackURL=%2F%2Fevil.test',
  '/reset-password/valid?callbackURL=%2Fdashboard',
  '/reset-password/valid?callbackURL=%2Fdashboard',
  '/reset-password/valid?callbackURL=%2Fdashboard%3Ftoken%3Dold%26keep%3D%7E%26token%3Dag%23frag',
  '/reset-password/unknown?callbackURL=%2Fdashboard%3Ferror%3Dold%26keep%3D%7E%26error%3Dag%23frag',
  '/reset-password/expired?callbackURL=%2Fdashboard',
  '/reset-password/boundary?callbackURL=%2Fdashboard',
  '/reset-password/duplicates?callbackURL=http%3A%2F%2Flocalhost%3A3000%2Fdone',
  '/delete-user/callback',
  '/reset-password/encoded%2Ftoken?callbackURL=%2Fdashboard',
  '/reset-password/literal%2Ftoken?callbackURL=%2Fdashboard',
  '/reset-password/%76alid?callbackURL=%2Fdashboard',
  '/reset-password/valid?callbackURL=%2Funicode-%E2%98%83%3Fx%3Dhello%2520world',
  '/delete-user/callback?callbackURL=a&callbackURL=b',
  '/delete-user/callback?token=',
  '/delete-user/callback?token=a&token=b',
  '/delete-user/callback?token=a&callbackURL=',
  '/delete-user/callback?token=a&callbackURL=x&callbackURL=y',
  '/delete-user/callback?token=a&callbackURL=https%3A%2F%2Fevil.test',
  '/delete-user/callback?token=a&callbackURL=%2Fdashboard',
])
  await compare(route);
await compare('/reset-password/valid?callbackURL=%2Fdashboard', undefined, {
  cookie: '',
  origin: 'https://evil.test',
  host: 'forged.test',
});
await compare(
  '/delete-user/callback?token=a&callbackURL=%2Fdashboard',
  undefined,
  { cookie: '', origin: 'https://evil.test', host: 'forged.test' },
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
function normalize(value) {
  if (Array.isArray(value)) return value.map(normalize);
  if (value && typeof value === 'object')
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [
        key,
        key === 'password'
          ? '[stored hash]'
          : key === 'id' && value.providerId
            ? `account:${value.userId}:${value.providerId}`
            : normalize(item),
      ]),
    );
  return value;
}
const mismatches = [];
for (let i = 0; i < expected.length; i++) {
  const want = expected[i];
  const observed = want.records
    ? { records: actual[i] }
    : {
        route: want.route,
        status: actual[i].status,
        body: actual[i].body,
        location: actual[i].headers['location']?.[0] ?? null,
        contentType: actual[i].headers['content-type']?.[0] ?? null,
      };
  if (want.records)
    for (const account of actual[i].accounts)
      if (account.providerId === 'credential')
        assert.match(account.password, /^[a-f0-9]{32}:[a-f0-9]{128}$/);
  try {
    assert.deepEqual(normalize(observed), normalize(want));
  } catch {
    mismatches.push({ actual: normalize(observed), expected: normalize(want) });
  }
}
assert.deepEqual(mismatches, []);
console.log(
  `Better Auth GET token callbacks: ${expected.length} actual HTTP/storage comparisons passed`,
);
