// Real configured HTTP handlers distinguish JOSE failures from claim-schema errors.
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
const RealDate = Date,
  now = 1800000000000;
const secret = 'better-auth-protocol-fixture-secret-32-characters';
const encoded = (value) => Buffer.from(value).toString('base64url');
const sign = (header, payload, key = secret) =>
  `${header}.${payload}.${createHmac('sha256', key).update(`${header}.${payload}`).digest('base64url')}`;
const cases = [];
const add = (header, payload = '{"email":"absent@fixture.test"}') =>
  cases.push({
    header,
    payload,
    token: sign(encoded(header), encoded(payload)),
  });
for (const header of [
  { alg: 'HS256' },
  { alg: 'HS256', typ: 'JWT' },
  { alg: 'HS384' },
  { alg: 'none' },
  {},
  { alg: 123 },
  { alg: 'HS256', crit: [] },
  { alg: 'HS256', crit: null },
  { alg: 'HS256', crit: ['unknown'], unknown: true },
  { alg: 'HS256', crit: ['b64'] },
  { alg: 'HS256', crit: ['b64'], b64: true },
  { alg: 'HS256', crit: ['b64', 'b64'], b64: true },
  { alg: 'HS256', crit: ['b64'], b64: false },
  { alg: 'HS256', crit: ['b64'], b64: 'true' },
  { alg: 'HS256', b64: false },
  { alg: 'HS256', b64: 123 },
])
  add(JSON.stringify(header));
for (const header of [
  'null',
  '[]',
  'true',
  '"header"',
  '{',
  '\uFEFF{"alg":"HS256"}',
  '{"alg":"wrong","alg":"HS256"}',
  '{"alg":"HS256","extra":1e400}',
])
  add(header);
for (const payload of [
  '{}',
  'null',
  '[]',
  'true',
  '"payload"',
  '{',
  '{"email":"invalid"}',
  '{"email":123}',
  '{"email":"ABSENT@FIXTURE.TEST"}',
  '{"email":"absent@fixture.test","updateTo":null}',
  '{"email":"absent@fixture.test","updateTo":""}',
  '{"email":"absent@fixture.test","updateTo":"new@fixture.test","requestType":"legacy"}',
  '{"email":"absent@fixture.test","requestType":123}',
  '\uFEFF{"email":"absent@fixture.test"}',
  '{"email":"invalid","email":"absent@fixture.test"}',
])
  add('{"alg":"HS256"}', payload);
for (const field of ['iat', 'nbf', 'exp'])
  for (const value of [
    'null',
    'true',
    '"1800000000"',
    '[]',
    '{}',
    '0',
    '1799999999',
    '1800000000',
    '1800000000.5',
    '1800000001',
    '1e400',
    '-1e400',
  ])
    add(
      '{"alg":"HS256"}',
      `{"email":"absent@fixture.test","${field}":${value}}`,
    );
for (const payload of [
  '{"email":"absent@fixture.test","nbf":1800000001,"exp":0}',
  '{"email":"absent@fixture.test","iat":null,"exp":0}',
])
  add('{"alg":"HS256"}', payload);
const header = encoded('{"alg":"HS256"}'),
  payload = encoded('{"email":"absent@fixture.test"}');
for (const [name, h, p] of [
  ['whitespace-header', ` ${header}\t`, payload],
  ['whitespace-payload', header, `${payload}\n`],
  ['padded-header', header + '==', payload],
  ['padded-payload', header, payload + '=='],
  ['bad-padding', header + '=', payload],
  ['invalid-utf8-header', '_w', payload],
  ['invalid-utf8-payload', header, '_w'],
])
  cases.push({
    name,
    payload: '{"email":"absent@fixture.test"}',
    token: sign(h, p),
  });
const padded = (text) => text + '='.repeat((4 - (text.length % 4)) % 4);
for (const [h, p] of [
  [padded(encoded('{"alg":"HS256","x":true}')), payload],
  [header, padded(payload)],
])
  cases.push({
    name: 'valid-padding',
    payload: '{"email":"absent@fixture.test"}',
    token: sign(h, p),
  });
const alphabet =
  'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
const noncanonical = sign(header, payload);
const last = alphabet.indexOf(noncanonical.at(-1));
cases.push({
  name: 'noncanonical-signature-bits',
  payload: '{"email":"absent@fixture.test"}',
  token: noncanonical.slice(0, -1) + alphabet[last | 1],
});
const valid = sign(header, payload);
for (const token of [
  valid + '=',
  valid + '\n',
  valid.slice(0, -1) + '!',
  valid + '.extra',
  valid.slice(0, valid.lastIndexOf('.')),
  sign(header, payload, 'wrong-secret'),
  '',
  '.',
  '..',
])
  cases.push({
    name: 'compact/signature',
    payload: '{"email":"absent@fixture.test"}',
    token,
  });
const expected = [];
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [now]));
  }
  static now() {
    return now;
  }
  static [Symbol.hasInstance](value) {
    return value instanceof RealDate;
  }
};
try {
  for (const test of cases) {
    const response = await reference.auth.handler(
      new Request(
        `http://localhost:3000/api/auth/verify-email?token=${encodeURIComponent(test.token)}`,
      ),
    );
    const text = await response.text();
    if (response.status === 500) {
      assert.equal(text, '');
      expected.push({ error: 'INVALID_CLAIMS' });
      continue;
    }
    assert.equal(response.status, 401, text);
    const body = JSON.parse(text);
    if (body.code === 'USER_NOT_FOUND') {
      const decoded = JSON.parse(test.payload.replace(/^\uFEFF/, ''));
      expected.push({
        claims: Object.fromEntries(
          ['email', 'updateTo', 'requestType']
            .filter((key) => Object.hasOwn(decoded, key))
            .map((key) => [key, decoded[key]]),
        ),
      });
    } else {
      assert.ok(['INVALID_TOKEN', 'TOKEN_EXPIRED'].includes(body.code), text);
      expected.push({ error: body.code });
    }
  }
} finally {
  globalThis.Date = RealDate;
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
    '--example',
    'email_token_parity',
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    input:
      cases
        .map((test) => JSON.stringify({ token: test.token, now }))
        .join('\n') + '\n',
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
  },
);
assert.equal(run.status, 0, run.stderr);
const actual = run.stdout
  .trim()
  .split('\n')
  .map((line) => JSON.parse(line));
assert.equal(actual.length, expected.length);
const mismatches = [];
for (let i = 0; i < cases.length; i++)
  try {
    assert.deepEqual(actual[i], expected[i]);
  } catch {
    mismatches.push({
      case: cases[i],
      actual: actual[i],
      expected: expected[i],
    });
  }
assert.deepEqual(mismatches, []);
console.log(
  `Better Auth email verification tokens: ${cases.length} actual JOSE/HTTP comparisons passed`,
);
