// Raw HTTP parser/schema/guard order compared with actual configured handlers.
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
const signup = {
  name: 'First Last',
  email: 'validation@fixture.test',
  password: '12345678',
};
const signin = { email: 'invalid-email', password: '12345678' };
const cases = [];
const add = (route, body, headers = {}, method = 'POST') =>
  cases.push({
    route,
    body: body === undefined ? undefined : JSON.stringify(body),
    headers,
    method,
  });
for (const body of [
  undefined,
  null,
  [],
  true,
  'string',
  {},
  {
    name: 123,
    email: 'invalid',
    password: '',
    image: null,
    rememberMe: 'false',
  },
  { ...signup, password: 'short' },
  { ...signup, password: 'x'.repeat(129) },
  signup,
  { ...signup, firstName: 'First' },
])
  add('/sign-up/email', body);
for (const body of [
  undefined,
  null,
  [],
  {},
  signin,
  { ...signin, rememberMe: 'false' },
  { ...signin, callbackURL: 123 },
  { ...signin, password: 'x'.repeat(129) },
])
  add('/sign-in/email', body);
for (const route of ['/verify-password', '/change-password', '/revoke-session'])
  for (const body of [undefined, null, [], {}]) add(route, body);
add('/verify-password', { password: 'x'.repeat(129) });
add('/change-password', {
  newPassword: '12345678',
  currentPassword: '12345678',
});
for (const headers of [
  { origin: 'https://evil.test' },
  {
    origin: undefined,
    'sec-fetch-site': 'cross-site',
    'sec-fetch-mode': 'navigate',
  },
  { origin: undefined, 'sec-fetch-mode': 'cors' },
  { origin: 'null', 'sec-fetch-site': 'same-origin' },
  { cookie: '', origin: undefined },
  { cookie: '', origin: 'https://evil.test' },
]) {
  add('/sign-in/email', signin, headers);
  add('/sign-in/email', {}, headers);
}
add('/sign-up/email', { ...signup, callbackURL: 'https://evil.test' });
add('/sign-in/email', { ...signin, errorCallbackURL: 'https://evil.test' });
add(
  '/sign-in/email?callbackURL=%2Fdashboard&callbackURL=https%3A%2F%2Fevil.test',
  signin,
);
for (const [route, method] of [
  ['/sign-in/email', 'GET'],
  ['/sign-up/email', 'HEAD'],
  ['/jwks', 'POST'],
  ['/token', 'DELETE'],
  ['/ok', 'HEAD'],
  ['/ok', 'OPTIONS'],
  ['/get-session', 'HEAD'],
  ['/unknown', 'POST'],
])
  add(route, undefined, {}, method);
for (const type of [
  'text/plain',
  'APPLICATION/JSON; charset=utf-8',
  'application/problem+json',
  'application/jsonfoo',
])
  add('/sign-in/email', signin, { 'content-type': type });
cases.push({
  route: '/sign-in/email',
  method: 'POST',
  headers: { 'content-type': 'application/x-www-form-urlencoded' },
  body: 'email=invalid-email&password=12345678&rememberMe=false',
});
cases.push({
  route: '/sign-in/email',
  method: 'POST',
  headers: { 'content-type': 'application/x-www-form-urlencoded' },
  body: 'email=first%40fixture.test&email=invalid-email&password=12345678',
});
cases.push({
  route: '/sign-in/email',
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: '{',
});
const inputs = [],
  expected = [];
for (const { route, method, headers, body } of cases) {
  const merged = {
    'content-type': 'application/json',
    origin: 'http://localhost:3000',
    ...headers,
  };
  for (const [name, value] of Object.entries(merged))
    if (value === undefined) delete merged[name];
  const request = new Request(`http://localhost:3000/api/auth${route}`, {
    method,
    headers: merged,
    ...(body === undefined ? {} : { body }),
  });
  const response = await reference.auth.handler(request);
  const text = await response.text();
  let value = text;
  try {
    value = JSON.parse(text);
  } catch {}
  expected.push({
    status: response.status,
    body: value,
    contentType: response.headers.get('content-type'),
  });
  inputs.push({
    op: 'request',
    now: 1800000000000,
    path: route,
    method,
    headers: Object.fromEntries(request.headers),
    ...(body === undefined ? {} : { body }),
  });
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
  .map((line) => JSON.parse(line));
assert.equal(actual.length, cases.length);
const mismatches = [];
for (let i = 0; i < cases.length; i++) {
  const observed = {
    status: actual[i].status,
    body: actual[i].body,
    contentType: actual[i].headers['content-type']?.[0] ?? null,
  };
  try {
    assert.deepEqual(observed, expected[i]);
  } catch {
    mismatches.push({
      case: cases[i],
      actual: observed,
      expected: expected[i],
    });
  }
}
assert.deepEqual(mismatches, []);
console.log(
  `Better Auth native credential HTTP validation: ${cases.length} actual parser/schema/guard/method comparisons passed`,
);
