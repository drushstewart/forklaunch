// Production rate-limit responses compared with configured Better Auth handlers.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
process.env.NODE_ENV = 'production';
const reference = await betterAuthReference(root);
const context = await reference.auth.$context;
assert.equal(context.rateLimit.enabled, true);
assert.equal(context.rateLimit.window, 10);
assert.equal(context.rateLimit.max, 100);
assert.equal(context.rateLimit.storage, 'memory');
const cases = [];
const start = 1800000000000;
const add = (offset, ip, route = '/sign-in/email', options = {}) =>
  cases.push({
    now: start + offset,
    path: route,
    method: 'POST',
    body: '{}',
    headers: {
      'content-type': 'application/json',
      origin: 'http://localhost:3000',
      ...(ip === undefined ? {} : { 'x-forwarded-for': ip }),
    },
    ...options,
  });
for (const time of [0, 5000, 9000, 10000, 18999, 19000]) add(time, '192.0.2.1');
for (let i = 0; i < 4; i++) add(20000, '192.0.2.1', '/sign-up/email');
for (const ip of [
  '2001:db8:1:2::1',
  '2001:db8:1:2::2',
  '2001:db8:1:2::3',
  '2001:db8:1:2::4',
  '2001:db8:1:3::1',
])
  add(30000, ip);
for (const ip of [
  '::ffff:192.0.2.2',
  '192.0.2.2',
  '::ffff:c000:202',
  '192.0.2.2',
])
  add(40000, ip);
for (const ip of [
  undefined,
  'invalid',
  '192.0.2.3,192.0.2.4',
  undefined,
  '192.0.2.5',
])
  add(50000, ip);
for (let i = 0; i < 101; i++)
  add(60000, '192.0.2.6', '/ok', { method: 'GET', body: undefined });
for (const time of [69999, 70000])
  add(time, '192.0.2.6', '/ok', { method: 'GET', body: undefined });
for (const [ip, route, options] of [
  ['192.0.2.7', '/sign-in/email', { method: 'GET', body: undefined }],
  ['192.0.2.8', '/sign-in/missing', {}],
  ['192.0.2.9', '/sign-in/email', { body: '{' }],
  [
    '192.0.2.10',
    '/sign-in/email',
    {
      headers: {
        'content-type': 'application/json',
        cookie: '',
        origin: 'https://evil.test',
        'x-forwarded-for': '192.0.2.10',
      },
    },
  ],
])
  for (let i = 0; i < 4; i++) add(80000, ip, route, options);
for (const time of [90000, 90000, 90000, 90000, 149999, 150000])
  add(time, '192.0.2.11', '/forget-password/missing');
for (const route of [
  '/sign-in/email/',
  '/sign-in/email?x=1',
  '/sign-in/email?x=2',
  '/sign-in/email/',
])
  add(160000, '192.0.2.12', route);
const RealDate = Date;
const expected = [];
for (const test of cases) {
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [test.now]));
    }
    static now() {
      return test.now;
    }
  };
  try {
    const response = await reference.auth.handler(
      new Request(`http://localhost:3000/api/auth${test.path}`, {
        method: test.method,
        headers: test.headers,
        ...(test.body === undefined ? {} : { body: test.body }),
      }),
    );
    let body = await response.text();
    try {
      body = JSON.parse(body);
    } catch {}
    expected.push({
      status: response.status,
      body,
      contentType: response.headers.get('content-type'),
      retry: response.headers.get('x-retry-after'),
    });
  } finally {
    globalThis.Date = RealDate;
  }
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
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
    input:
      cases
        .map((test) => JSON.stringify({ op: 'request', ...test }))
        .join('\n') + '\n',
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
    retry: actual[i].headers['x-retry-after']?.[0] ?? null,
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
  `Better Auth production rate limits: ${cases.length} actual HTTP comparisons passed`,
);
