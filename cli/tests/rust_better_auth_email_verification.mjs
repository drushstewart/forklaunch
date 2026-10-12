// Actual signed email verification, email-change flows, cookies and redirects.
import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';
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
const seedIds = new Set(reference.database.session.map((row) => row.id));
const normalizeRecords = (records) => {
  let n = 0;
  return {
    ...records,
    sessions: records.sessions.map((row) =>
      seedIds.has(row.id)
        ? row
        : { ...row, id: `created:${++n}`, token: `created-token:${n}` },
    ),
  };
};
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
    location: response.headers.get('location'),
    contentType: response.headers.get('content-type'),
  });
  inputs.push({ op: 'records', now });
  expected.push({ records: normalizeRecords(snapshot()) });
}
const secret = 'better-auth-protocol-fixture-secret-32-characters';
const sign = (claims) => {
  const input =
    Buffer.from(JSON.stringify({ alg: 'HS256' })).toString('base64url') +
    '.' +
    Buffer.from(JSON.stringify(claims)).toString('base64url');
  return (
    input + '.' + createHmac('sha256', secret).update(input).digest('base64url')
  );
};
const route = (claims, callback) =>
  '/verify-email?token=' +
  encodeURIComponent(typeof claims === 'string' ? claims : sign(claims)) +
  (callback === undefined
    ? ''
    : '&callbackURL=' + encodeURIComponent(callback));
const claims = { email: 'profile@fixture.test', exp: 1800000600 };
const foreignHeaders = {
  ...signed,
  cookie: [...foreign.cookies]
    .map(([name, value]) => `${name}=${value}`)
    .join('; '),
};
for (const path of [
  '/verify-email',
  '/verify-email?callbackURL=a&callbackURL=b',
  '/verify-email?token=a&token=b',
  '/verify-email?token=bad&callbackURL=https%3A%2F%2Fevil.test',
  route('bad'),
  route({ ...claims, exp: 0 }),
  route({ email: 'absent@fixture.test' }),
  route({ email: 'invalid' }),
  route('bad', '/done?error=old&x=hello%20world#frag'),
  route({ ...claims, exp: 0 }, '/done?'),
  route({ email: 'invalid' }, '/done'),
])
  await compare(path);
await compare(route(claims));
await compare(route(claims));
await compare(route({ email: 'foreign@fixture.test' }), undefined, signed);
await compare(
  route({
    ...claims,
    updateTo: 'Changed@fixture.test',
    requestType: 'change-email-confirmation',
  }),
);
await compare(
  route({
    ...claims,
    updateTo: 'Changed@fixture.test',
    requestType: 'change-email-confirmation',
  }),
  undefined,
  foreignHeaders,
);
await compare(
  route({
    ...claims,
    updateTo: 'Changed@fixture.test',
    requestType: 'change-email-verification',
  }),
);
await compare(
  route({
    email: 'changed@fixture.test',
    updateTo: 'legacy@fixture.test',
    requestType: 'legacy',
  }),
);
await compare(route({ email: 'legacy@fixture.test', updateTo: '' }));
await compare(route({ email: 'legacy@fixture.test' }, '/done?x=~#frag'));
await compare(route({ email: 'legacy@fixture.test' }, '/snowman-☃'));
await compare(route({ email: 'legacy@fixture.test' }, '/café'));
await compare(
  route({ email: 'legacy@fixture.test' }, ' \thttp://localhost:3000/done\t '),
);
await compare(
  route({ email: 'legacy@fixture.test' }, '\r\nhttp://localhost:3000/done\r\n'),
);
const onlyFlag = {
  ...foreignHeaders,
  cookie: [...foreign.cookies]
    .filter(([name]) => name.includes('dont_remember'))
    .map(([name, value]) => `${name}=${value}`)
    .join('; '),
  'x-forwarded-for': '2001:db8:1:2::3',
  'user-agent': 'Email-fixture',
};
await compare(
  route({
    email: 'foreign@fixture.test',
    updateTo: 'New-Foreign@fixture.test',
    requestType: 'change-email-verification',
  }),
  undefined,
  onlyFlag,
);
await compare(
  route({
    email: 'new-foreign@fixture.test',
    updateTo: 'legacy-foreign@fixture.test',
  }),
  undefined,
  { 'content-type': 'application/json' },
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
    ? { records: normalizeRecords(actual[i]) }
    : {
        route: want.route,
        status: actual[i].status,
        body: actual[i].body,
        cookies: attributes(actual[i].headers['set-cookie'] ?? []),
        location: actual[i].headers['location']?.[0] ?? null,
        contentType: actual[i].headers['content-type']?.[0] ?? null,
      };
  try {
    assert.deepEqual(observed, want);
  } catch {
    mismatches.push({ actual: observed, expected: want });
  }
}
assert.deepEqual(mismatches, []);
console.log(
  `Better Auth signed email verification HTTP: ${expected.length} actual response/storage comparisons passed`,
);
