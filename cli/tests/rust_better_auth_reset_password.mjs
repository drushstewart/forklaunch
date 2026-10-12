// Actual single-use token reset semantics, including persisted consumption.
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
const ownerCredential = reference.database.account.find(
  (row) => row.userId === ownerId,
);
reference.database.account.push({
  ...ownerCredential,
  id: 'legacy-owner-credential',
  accountId: 'legacy-owner-subject',
});
reference.database.account.push({
  ...ownerCredential,
  id: 'legacy-social-credential',
  userId: socialId,
  accountId: 'legacy-social-subject',
});

for (const [id, token, user, created, expires] of [
  ['valid', 'valid', ownerId, start, start + 60000],
  ['expired', 'expired', ownerId, start, start - 1],
  ['missing-user', 'missing-user', 'absent', start, start + 60000],
  ['older-duplicate', 'duplicates', 'absent', start - 1, start + 60000],
  ['newer-duplicate', 'duplicates', ownerId, start, start + 60000],
  ['boundary', 'boundary', ownerId, start, start],
  ['social', 'social', socialId, start, start + 60000],
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
    method: 'POST',
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
    method: 'POST',
    headers: Object.fromEntries(request.headers),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  expected.push({ route, status: response.status, body: value });
  inputs.push({ op: 'records', now: start, withVerifications: true });
  expected.push({ records: snapshot() });
}
for (const body of [
  undefined,
  null,
  [],
  {},
  { newPassword: 123, token: 123 },
  { newPassword: 'short' },
  { newPassword: 'short', token: 'unknown' },
  { newPassword: 'x'.repeat(129), token: 'unknown' },
  { newPassword: '12345678', token: 'unknown' },
])
  await compare('/reset-password', body);
await compare('/reset-password?token=a&token=b', {});
await compare('/reset-password?token=a&token=b', { newPassword: '12345678' });
await compare('/reset-password', { newPassword: '12345678', token: 'expired' });
await compare('/reset-password', {
  newPassword: '12345678',
  token: 'missing-user',
});
await compare('/reset-password', { newPassword: '12345678', token: 'valid' });
await compare('/reset-password', { newPassword: '12345678', token: 'valid' });
await compare('/reset-password?token=duplicates', {
  newPassword: 'password-via-query',
  token: '',
});
await compare('/reset-password?token=boundary', {
  newPassword: 'boundary-password',
});
await compare('/reset-password', {
  newPassword: 'social-password',
  token: 'social',
});
for (const [who, passwords] of [
  [owner, ['test-only-password', '12345678', 'boundary-password']],
  [social, ['social-password']],
]) {
  const signed = {
    ...headers,
    origin: 'http://localhost:3000',
    cookie: [...who.cookies]
      .map(([name, value]) => `${name}=${value}`)
      .join('; '),
  };
  for (const password of passwords)
    await compare('/verify-password', { password }, signed);
}
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
function normalize(value, hashes = new Map()) {
  if (Array.isArray(value)) return value.map((item) => normalize(item, hashes));
  if (value && typeof value === 'object')
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => {
        if (key === 'password') {
          if (!hashes.has(item))
            hashes.set(item, `[stored hash:${hashes.size + 1}]`);
          return [key, hashes.get(item)];
        }
        if (key === 'id' && value.providerId)
          return [key, `account:${value.userId}:${value.providerId}`];
        return [key, normalize(item, hashes)];
      }),
    );
  return value;
}
const mismatches = [];
for (let i = 0; i < expected.length; i++) {
  const want = expected[i];
  const observed = want.records
    ? { records: actual[i] }
    : { route: want.route, status: actual[i].status, body: actual[i].body };
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
  `Better Auth token password reset: ${expected.length} actual HTTP/storage comparisons passed`,
);
