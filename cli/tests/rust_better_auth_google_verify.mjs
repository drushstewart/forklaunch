// Actual Google RS256 ID-token verifier with its certificate-fetch boundary owned.
import assert from 'node:assert/strict';
import { generateKeyPairSync, sign } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const { verifyGoogleIdToken } = await import(
  pathToFileURL(
    path.join(
      process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES,
      'node_modules/@better-auth/core/dist/social-providers/google.mjs'
    )
  )
);
const pair = generateKeyPairSync('rsa', { modulusLength: 2048 }),
  other = generateKeyPairSync('rsa', { modulusLength: 2048 });
const key = {
  ...pair.publicKey.export({ format: 'jwk' }),
  kid: 'google-fixture-key',
  alg: 'RS256',
  use: 'sig'
};
const start = 1800000000,
  RealDate = Date,
  clientId = 'fixture-google-id';
const base = {
  iss: 'https://accounts.google.com',
  aud: clientId,
  iat: start,
  exp: start + 300,
  sub: 'google-subject',
  email: 'google@example.test',
  nonce: 'fixture-nonce'
};
function token(payload = base, header = {}, privateKey = pair.privateKey) {
  const encode = (value) =>
    Buffer.from(
      typeof value === 'string' ? value : JSON.stringify(value)
    ).toString('base64url');
  const content =
    encode({ alg: 'RS256', kid: key.kid, ...header }) + '.' + encode(payload);
  return (
    content +
    '.' +
    sign('sha256', Buffer.from(content), privateKey).toString('base64url')
  );
}
let currentKeys;
const originalFetch = globalThis.fetch;
globalThis.fetch = async (input) => {
  const url =
    typeof input === 'string'
      ? input
      : input instanceof URL
        ? input.href
        : input.url;
  assert.equal(url, 'https://www.googleapis.com/oauth2/v3/certs');
  return new Response(JSON.stringify(currentKeys), {
    headers: { 'content-type': 'application/json' }
  });
};
const inputs = [],
  expected = [];
async function compare({
  signed = token(),
  nonce = 'fixture-nonce',
  jwks = { keys: [key] },
  now = start,
  audience = clientId
} = {}) {
  const input = { token: signed, clientId: audience, nonce, jwks, now };
  inputs.push(input);
  currentKeys = jwks;
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [now * 1000]));
    }
    static now() {
      return now * 1000;
    }
  };
  try {
    expected.push(
      JSON.parse(
        JSON.stringify({
          payload: await verifyGoogleIdToken({ token: signed, audience, nonce })
        })
      )
    );
  } finally {
    globalThis.Date = RealDate;
  }
}
try {
  await compare();
  for (const patch of [
    { iss: 'accounts.google.com' },
    { iss: 'wrong' },
    { iss: null },
    { aud: [clientId] },
    { aud: ['other', clientId, 1] },
    { aud: [] },
    { aud: null },
    { sub: '' },
    { sub: null },
    { sub: 1 },
    { email: null }
  ])
    await compare({ signed: token({ ...base, ...patch }) });
  for (const field of ['iat', 'nbf', 'exp']) {
    for (const value of [
      null,
      '1',
      true,
      [],
      {},
      0,
      -1,
      start - 3601,
      start - 3600,
      start - 3599.9,
      start - 0.1,
      start,
      start + 0.1,
      start + 1
    ])
      await compare({ signed: token({ ...base, [field]: value }) });
    const payload = { ...base };
    delete payload[field];
    await compare({ signed: token(payload) });
    for (const value of ['1e400', '-1e400', '-0.0'])
      await compare({
        signed: token(
          JSON.stringify({ ...base, [field]: 0 }).replace(
            new RegExp('"' + field + '":[^,}]+'),
            '"' + field + '":' + value
          )
        )
      });
  }
  for (const nonce of [undefined, null, '', 'wrong', 'fixture-nonce'])
    await compare({ nonce });
  for (const patch of [
    { kid: undefined },
    { kid: '' },
    { kid: null },
    { kid: 0 },
    { kid: false },
    { kid: true },
    { kid: 'unknown' },
    { alg: 'RS384' },
    { alg: 'PS256' },
    { alg: 'HS256' },
    { alg: null },
    { crit: ['unknown'] },
    { crit: ['b64'], b64: true },
    { crit: ['b64'], b64: false },
    { b64: false }
  ])
    await compare({ signed: token(base, patch) });
  for (const payload of [
    '"string"',
    'null',
    '[]',
    '{}',
    '{"iss":"https://accounts.google.com"}',
    '{invalid'
  ])
    await compare({ signed: token(payload) });
  for (const signed of [
    '',
    'a',
    'a.b',
    'a.b.c',
    'a.b.c.d',
    token().slice(0, -8),
    token(base, {}, other.privateKey)
  ])
    await compare({ signed });
  for (const patch of [
    { use: 'enc' },
    { alg: 'HS256' },
    { kid: 'different' },
    { key_ops: ['verify'] },
    { key_ops: [] },
    { key_ops: ['sign'] },
    { key_ops: ['verify', 'verify'] },
    { ext: false },
    { ext: null },
    { d: null },
    { d: '' },
    { priv: 'private' },
    { priv: null },
    { n: 'bad' },
    { e: 'bad' },
    { n: key.n + '!' },
    { e: key.e + '=' }
  ])
    await compare({ jwks: { keys: [{ ...key, ...patch }] } });
  for (const jwks of [
    {},
    { keys: [] },
    { keys: null },
    { keys: {} },
    { keys: [null] },
    { keys: [key, key] },
    { keys: [{ ...key, n: null }, key] },
    { keys: [key, { ...key, n: null }] },
    { keys: [{ ...key, n: 'bad' }, key] },
    {
      keys: [
        { ...pair.privateKey.export({ format: 'jwk' }), kid: key.kid },
        key
      ]
    }
  ])
    await compare({ jwks });
  await compare({ jwks: { keys: [{ ...key, kid: 'other' }, key] } });
  for (const patch of [
    { n: '' },
    { n: null },
    { e: '' },
    { e: null },
    { n: 42 },
    { n: {} },
    { key_ops: [] },
    { key_ops: ['sign'] },
    { ext: null },
    { d: null },
    { priv: 'private' }
  ])
    await compare({ jwks: { keys: [{ ...key, ...patch }, key] } });
  const privateJwk = pair.privateKey.export({ format: 'jwk' });
  for (const patch of [
    {},
    { d: 'bad' },
    { p: null },
    { q: '' },
    { dp: 'bad' },
    { qi: null },
    { key_ops: ['verify'] },
    { key_ops: [] }
  ])
    await compare({
      jwks: { keys: [{ ...privateJwk, kid: key.kid, ...patch }, key] }
    });
  await compare({ audience: 'wrong-client' });
} finally {
  globalThis.fetch = originalFetch;
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
    'google_verify_parity'
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    input: inputs.map((value) => JSON.stringify(value)).join('\n') + '\n',
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024
  }
);
assert.equal(run.status, 0, run.stderr);
const results = run.stdout
  .trim()
  .split('\n')
  .map((line) => JSON.parse(JSON.stringify(JSON.parse(line))));
assert.equal(results.length, expected.length);
const differences = [];
for (let i = 0; i < results.length; i++)
  try {
    assert.deepEqual(results[i], expected[i]);
  } catch (error) {
    differences.push({ case: i, input: inputs[i], diff: error.message });
  }
assert.deepEqual(differences.slice(0, 10), []);
console.log(
  `Better Auth Google ID-token verification: ${inputs.length} actual provider comparisons passed`
);
