// Execute the configured server-only verifier against independently signed tokens.
import assert from 'node:assert/strict';
import { generateKeyPairSync, sign } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const reference = await betterAuthReference(root),
  RealDate = Date,
  start = 1800000000;
async function atTime(now, work) {
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [now * 1000]));
    }
    static [Symbol.hasInstance](value) {
      return value instanceof RealDate;
    }
    static now() {
      return now * 1000;
    }
  };
  try {
    return await work();
  } finally {
    globalThis.Date = RealDate;
  }
}
const { privateKey, publicKey } = generateKeyPairSync('ed25519');
const row = {
  id: 'verify-fixture-key',
  alg: 'EdDSA',
  crv: 'Ed25519',
  publicKey: JSON.stringify(publicKey.export({ format: 'jwk' })),
  privateKey: 'not-needed-for-verification',
  createdAt: new RealDate(start * 1000),
  expiresAt: new RealDate((start - 40 * 86400) * 1000)
};
reference.database.jwks.push(row);
const inputs = [{ op: 'seed', now: start, row: { ...row } }],
  expected = [];
async function compare(body, now = start) {
  let result;
  try {
    result = {
      body: await atTime(now, () => reference.auth.api.verifyJWT({ body }))
    };
  } catch (error) {
    result = {
      error: {
        code: error.body?.code,
        message: error.body?.message ?? error.message
      }
    };
  }
  inputs.push({ op: 'verify', now, ...(body === undefined ? {} : { body }) });
  expected.push(result);
}
function token(payload, header = { alg: 'EdDSA', kid: row.id }) {
  const encode = (value) =>
    Buffer.from(
      typeof value === 'string' ? value : JSON.stringify(value)
    ).toString('base64url');
  const content = encode(header) + '.' + encode(payload);
  return (
    content +
    '.' +
    sign(null, Buffer.from(content), privateKey).toString('base64url')
  );
}
const base = {
  iss: 'http://localhost:3000',
  aud: 'http://localhost:3000',
  sub: 'fixture-user'
};
for (const body of [
  undefined,
  null,
  {},
  { token: null },
  { token: 1 },
  { token: [] },
  { token: '', issuer: null },
  { token: '', issuer: 1 }
])
  await compare(body);
for (const malformed of ['', 'a', 'a.b', 'a.b.c', 'a.b.c.d', 'e30.e30.', '..'])
  await compare({ token: malformed });
for (const payload of [
  base,
  { ...base, exp: start + 1 },
  { ...base, exp: start },
  { ...base, exp: start - 1 },
  { ...base, exp: start + 0.1 },
  { ...base, nbf: start },
  { ...base, nbf: start + 0.1 },
  { ...base, iat: start + 999999 },
  { ...base, iat: -999999 },
  { ...base, sub: '' },
  { ...base, sub: 0 },
  { ...base, sub: false },
  { ...base, sub: null },
  { ...base, sub: [] },
  { ...base, sub: {} },
  { ...base, sub: 1 },
  { ...base, aud: ['http://localhost:3000'] },
  { ...base, aud: ['other', 'http://localhost:3000', 1] },
  { ...base, aud: [] },
  { ...base, aud: 1 },
  { ...base, iss: 'other' },
  { ...base, iss: null },
  { ...base, extra: { $serde_json$: 'literal' } },
  {},
  [],
  null,
  false,
  1,
  '"text"'
])
  await compare({ token: token(payload) });
for (const field of ['exp', 'nbf', 'iat'])
  for (const value of [null, '1', true, [], {}, -0, 0, 1.5])
    await compare({ token: token({ ...base, [field]: value }) });
for (const field of ['exp', 'nbf', 'iat'])
  for (const value of ['1e400', '-1e400', '0.0', '-0.0'])
    await compare({
      token: token(JSON.stringify(base).slice(0, -1) + `,"${field}":${value}}`)
    });
for (const header of [
  { alg: 'EdDSA' },
  { alg: 'Ed25519', kid: row.id },
  { alg: 'none', kid: row.id },
  { alg: 'HS256', kid: row.id },
  { alg: 'EdDSA', kid: 'absent' },
  { alg: 'EdDSA', kid: 1 },
  { alg: 'EdDSA', kid: '' },
  { alg: 'EdDSA', kid: row.id, typ: 'custom' },
  { alg: 'EdDSA', kid: row.id, b64: true },
  { alg: 'EdDSA', kid: row.id, b64: false },
  { alg: 'EdDSA', kid: row.id, crit: [] },
  { alg: 'EdDSA', kid: row.id, crit: ['b64'], b64: true },
  { alg: 'EdDSA', kid: row.id, crit: ['b64'], b64: false },
  { alg: 'EdDSA', kid: row.id, crit: ['b64', 'b64'], b64: true },
  { alg: 'EdDSA', kid: row.id, crit: ['unknown'], unknown: true },
  [],
  null
])
  await compare({ token: token(base, header) });
const valid = token(base);
await compare({ token: valid.slice(0, -8) + 'AAAAAAAA' });
for (const issuer of [undefined, '', 'http://localhost:3000', 'other'])
  await compare({ token: valid, ...(issuer === undefined ? {} : { issuer }) });
await compare({ token: token({ ...base, iss: 'other' }), issuer: 'other' });
const parts = valid.split('.');
for (const suffix of ['=', '==', '\n', ' ', '!', '+', '/'])
  await compare({
    token: parts.slice(0, 2).join('.') + '.' + parts[2] + suffix
  });
// getAllKeys applies the adapter's first-100 limit, without retired-key visibility filtering.
for (let i = 0; i < 105; i++) {
  const next = { ...row, id: 'verify-bulk-' + i };
  reference.database.jwks.push(next);
  inputs.push({ op: 'seed', now: start, row: next });
  expected.push({ ok: true });
}
for (const kid of ['verify-bulk-98', 'verify-bulk-99', 'verify-bulk-104'])
  await compare({ token: token(base, { alg: 'EdDSA', kid }) });
const publicJwk = publicKey.export({ format: 'jwk' });
for (const patch of [
  { use: 'sig' },
  { use: 'enc' },
  { use: null },
  { use: true },
  { key_ops: ['verify'] },
  { key_ops: ['sign'] },
  { key_ops: [] },
  { key_ops: ['verify', 'verify'] },
  { key_ops: ['verify', 'sign'] },
  { key_ops: null },
  { key_ops: 'verify' },
  { ext: true },
  { ext: false },
  { ext: null },
  { ext: 'false' },
  { alg: 'EdDSA' },
  { alg: 'HS256' },
  { alg: null },
  { kid: 'ignored-public-kid' },
  { crv: 'Ed448' },
  { x: 'malformed' },
  { d: privateKey.export({ format: 'jwk' }).d },
  { d: 'malformed' },
  { d: null },
  { d: '' },
  { priv: 'private-material' },
  { x: publicJwk.x + '=' },
  { x: publicJwk.x + '==' },
  { x: Buffer.from(publicJwk.x, 'base64url').toString('base64') },
  { x: publicJwk.x + '!' },
  { x: ' !' + publicJwk.x + '==suffix' },
  { x: publicJwk.x + 'a' }
]) {
  row.publicKey = JSON.stringify({ ...publicJwk, ...patch });
  inputs.push({ op: 'replace', now: start, row: { ...row } });
  expected.push({ ok: true });
  await compare({ token: valid });
}
row.publicKey = JSON.stringify(publicJwk);
for (const alg of ['EdDSA', 'Ed25519', null, '', 'HS256', 1, 'RS256']) {
  row.alg = alg;
  inputs.push({ op: 'replace', now: start, row: { ...row } });
  expected.push({ ok: true });
  await compare({ token: valid });
  await compare({ token: token(base, { alg: 'Ed25519', kid: row.id }) });
}
for (const [algorithm, namedCurve, hash] of [
  ['ES256', 'prime256v1', 'sha256'],
  ['ES384', 'secp384r1', 'sha384'],
  ['ES512', 'secp521r1', 'sha512']
]) {
  const pair = generateKeyPairSync('ec', { namedCurve });
  const publicJwk = pair.publicKey.export({ format: 'jwk' });
  Object.assign(row, {
    alg: algorithm,
    crv: publicJwk.crv,
    publicKey: JSON.stringify(publicJwk)
  });
  function ecToken(payload, header = { alg: algorithm, kid: row.id }) {
    const content =
      Buffer.from(JSON.stringify(header)).toString('base64url') +
      '.' +
      Buffer.from(JSON.stringify(payload)).toString('base64url');
    return (
      content +
      '.' +
      sign(hash, Buffer.from(content), {
        key: pair.privateKey,
        dsaEncoding: 'ieee-p1363'
      }).toString('base64url')
    );
  }
  const valid = ecToken(base);
  for (const patch of [
    {},
    { use: 'enc' },
    { alg: 'HS256' },
    { kid: 'ignored' },
    { key_ops: ['verify'] },
    { key_ops: ['sign'] },
    { ext: false },
    { ext: null },
    { d: null },
    { crv: 'P-521' },
    { x: 'bad' },
    { y: 'bad' },
    { x: publicJwk.x + '!' },
    { y: publicJwk.y + '=' }
  ]) {
    row.publicKey = JSON.stringify({ ...publicJwk, ...patch });
    inputs.push({ op: 'replace', now: start, row: { ...row } });
    expected.push({ ok: true });
    await compare({ token: valid });
  }
  row.publicKey = JSON.stringify(publicJwk);
  for (const rowAlg of [
    algorithm,
    null,
    'EdDSA',
    algorithm === 'ES256' ? 'ES384' : 'ES256'
  ]) {
    row.alg = rowAlg;
    inputs.push({ op: 'replace', now: start, row: { ...row } });
    expected.push({ ok: true });
    await compare({ token: valid });
  }
  row.alg = algorithm;
  inputs.push({ op: 'replace', now: start, row: { ...row } });
  expected.push({ ok: true });
  for (const payload of [
    base,
    { ...base, exp: start },
    { ...base, exp: start + 1 },
    { ...base, nbf: start + 1 },
    { ...base, sub: '' }
  ])
    await compare({ token: ecToken(payload) });
  for (const alg of [
    'EdDSA',
    algorithm === 'ES256' ? 'ES384' : 'ES256',
    'HS256'
  ])
    await compare({ token: ecToken(base, { alg, kid: row.id }) });
  await compare({ token: valid.slice(0, -8) });
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
    'jwt_helper_parity'
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
const actual = run.stdout
  .trim()
  .split('\n')
  .map((line) => JSON.parse(line))
  .slice(1);
assert.equal(actual.length, expected.length);
const differences = [];
for (let i = 0; i < expected.length; i++)
  try {
    assert.deepEqual(actual[i], expected[i]);
  } catch (error) {
    differences.push({ case: i, input: inputs[i + 1], diff: error.message });
  }
assert.deepEqual(differences.slice(0, 10), []);
console.log(
  `Better Auth server-only verifyJWT: ${inputs.filter((input) => input.op === 'verify').length} actual API and ${inputs.filter((input) => input.op !== 'verify').length - 1} key-store comparisons passed`
);
