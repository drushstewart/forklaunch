// Actual server-only signing, claims/options/errors and persisted key side effects.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const reference = await betterAuthReference(root),
  RealDate = Date,
  start = 1800000000;
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
const jose = await import(
  pathToFileURL(
    path.join(dependencies, 'node_modules/jose/dist/webapi/index.js')
  )
);
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
const inputs = [],
  expected = [];
async function compare(body, now = start) {
  let result;
  try {
    result = {
      body: await atTime(now, () => reference.auth.api.signJWT({ body }))
    };
  } catch (error) {
    result = {
      error: {
        ...(error.body?.code ? { code: error.body.code } : {}),
        message: error.body?.message ?? error.message
      }
    };
  }
  inputs.push({ op: 'sign', now, ...(body === undefined ? {} : { body }) });
  expected.push(result);
  inputs.push({ op: 'records', now });
  expected.push(JSON.parse(JSON.stringify(reference.database.jwks)));
}
for (const body of [
  undefined,
  null,
  {},
  { payload: null },
  { payload: [] },
  { payload: {}, overrideOptions: null },
  { payload: {}, overrideOptions: [] },
  { payload: {}, overrideOptions: { jwt: { expirationTime: 'bad' } } },
  { payload: { exp: 'bad' } }
])
  await compare(body);
for (const payload of [
  {},
  { sub: 'user' },
  { sub: 'user', email: 'fixture@example.test' },
  { iat: start },
  { iat: 0 },
  { iat: null },
  { iat: false },
  { iat: '1h' },
  { iat: 'bad' },
  { iat: -1 },
  { iat: 1.5 },
  { exp: start + 10 },
  { exp: 0 },
  { exp: null },
  { exp: false },
  { exp: '1h' },
  { exp: 'bad' },
  { nbf: 0 },
  { nbf: null },
  { nbf: false },
  { nbf: '1h' },
  { nbf: 'bad' },
  { iss: 'custom' },
  { iss: '' },
  { iss: null },
  { iss: 1 },
  { aud: 'custom' },
  { aud: ['one', 'two'] },
  { aud: [] },
  { aud: null },
  { aud: 1 },
  { aud: [1] },
  { sub: '' },
  { sub: null },
  { sub: 0 },
  { sub: false },
  { sub: 1 },
  { jti: '' },
  { jti: null },
  { jti: false },
  { jti: 1 },
  { jti: 'custom' },
  {
    2: 'numeric-first',
    1: 'first',
    extra: { nested: true },
    __proto__: 'ignored'
  },
  { iat: false, exp: 10 },
  { iat: 'bad', exp: 10 },
  { iat: '1h', exp: 10 },
  { iat: null, exp: 10 }
])
  await compare({ payload });
for (const overrideOptions of [
  {},
  { jwt: null },
  { jwt: {} },
  { jwt: { issuer: 'custom', audience: ['one', 'two'] } },
  { jwt: { issuer: null, audience: null } },
  { jwt: { issuer: 1 } },
  { jwt: { audience: 1 } },
  { jwt: { expirationTime: 10 } },
  { jwt: { expirationTime: 0 } },
  { jwt: { expirationTime: null } },
  { jwt: { expirationTime: '1 hour' } },
  { jwt: { expirationTime: '1.5s' } },
  { jwt: { expirationTime: '1mo' } },
  { jwt: { expirationTime: '-1s' } },
  { jwt: { expirationTime: '1 week ago' } },
  { jwt: { expirationTime: 'bad' } },
  { jwt: { expirationTime: true } },
  { jwt: { expirationTime: {} } },
  { jwks: { keyPairConfig: { alg: 'EdDSA' } } },
  { jwks: { keyPairConfig: { alg: 'Ed25519' } } },
  { jwks: { keyPairConfig: { alg: 'RS256' } } }
])
  await compare({ payload: { sub: 'user' }, overrideOptions });
await compare({
  payload: { exp: 10 },
  overrideOptions: { jwt: { expirationTime: 'bad' } }
});
await compare({
  payload: { sub: 'user', iss: 'payload-issuer', aud: 'payload-audience' },
  overrideOptions: {
    jwt: { issuer: 'options-issuer', audience: 'options-audience' }
  }
});
for (const text of [
  '1s',
  '-1.5s',
  '1.5s',
  '1 day AGO',
  '1 day ago',
  '1 day FROM NOW',
  '1S',
  '1mo',
  '+ 2 min',
  '1s\n',
  '1s\r\n',
  '1s\n\n',
  '١s',
  '1ſ',
  '9'.repeat(400) + 'years'
]) {
  await compare({
    payload: { sub: 'user' },
    overrideOptions: { jwt: { expirationTime: text } }
  });
  await compare({ payload: { sub: 'user', exp: text } });
}
for (const payload of [
  { sub: 1, nbf: 'bad' },
  { iat: 'bad', sub: 1, exp: 10 },
  { nbf: 'bad', jti: 1 },
  { iss: 1, aud: 1 },
  { exp: 'bad', iss: 1 },
  { iat: [], exp: 10 },
  { iat: {}, exp: 10 },
  { iat: { toString: 'bad' }, exp: 10 },
  { nbf: [], exp: 10 },
  { nbf: {}, exp: 10 }
])
  await compare({ payload });
for (const interval of [60, 0, -1, 0.0015, null]) {
  reference.database.jwks.splice(0);
  inputs.push({ op: 'clear', now: start });
  expected.push({ cleared: true });
  inputs.push({ op: 'records', now: start });
  expected.push([]);
  await compare({
    payload: { sub: 'rotating-user' },
    overrideOptions: { jwks: { rotationInterval: interval } }
  });
  await compare(
    {
      payload: { sub: 'rotating-user' },
      overrideOptions: { jwks: { rotationInterval: interval } }
    },
    start + 1
  );
}
for (const config of [
  { alg: 'Ed25519' },
  { alg: 'EdDSA', crv: 'Ed25519' },
  { alg: 'ES256' },
  { alg: 'ES256', crv: 'P-256' },
  { alg: 'ES384' },
  { alg: 'ES384', crv: 'P-384' },
  { alg: 'ES512' },
  { alg: 'ES512', crv: 'P-521' },
  { alg: 'ES512', crv: 'P-384' },
  { alg: 'EdDSA', crv: 'Ed448' },
  { alg: 'ES256', crv: 'P-384' },
  { alg: 'ES384', crv: 'P-256' }
]) {
  reference.database.jwks.splice(0);
  inputs.push({ op: 'clear', now: start });
  expected.push({ cleared: true });
  inputs.push({ op: 'records', now: start });
  expected.push([]);
  const overrideOptions = { jwks: { keyPairConfig: config } };
  await compare({ payload: { sub: 'profile-user' }, overrideOptions });
  await compare(
    { payload: { sub: 'profile-user', extra: [1, 'value'] }, overrideOptions },
    start + 1
  );
  if (
    !config.crv ||
    (['Ed25519', 'P-256', 'P-384'].includes(config.crv) &&
      config.alg !== 'ES256' &&
      config.alg !== 'ES384')
  )
    await compare({ payload: { sub: 'fallback-user' } });
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
  .map((line) => JSON.parse(line));
assert.equal(actual.length, expected.length);
async function normalized(values) {
  const result = [];
  for (let i = 0; i < values.length; i += 2) {
    const response = values[i],
      rows = values[i + 1];
    if (response.body?.token) {
      const header = jose.decodeProtectedHeader(response.body.token),
        key = rows.find((row) => row.id === header.kid);
      assert.ok(key, 'Signed token must reference a persisted key');
      const verified = await jose.compactVerify(
        response.body.token,
        await jose.importJWK(JSON.parse(key.publicKey), key.alg ?? 'EdDSA')
      );
      const payload = JSON.parse(
        new TextDecoder('utf-8', { fatal: true }).decode(verified.payload)
      );
      result.push({
        body: {
          header: { ...header, kid: `[key:${rows.indexOf(key)}]` },
          payload
        }
      });
    } else result.push(response);
    result.push(
      rows.map((row) => ({
        alg: row.alg,
        crv: row.crv,
        createdAt: row.createdAt,
        expiresAt: row.expiresAt ?? null
      }))
    );
  }
  return result;
}
const [nativeValues, actualValues] = await Promise.all([
  normalized(actual),
  normalized(expected)
]);
const differences = [];
for (let i = 0; i < expected.length; i++)
  try {
    assert.deepEqual(nativeValues[i], actualValues[i]);
  } catch (error) {
    differences.push({ case: i, input: inputs[i], diff: error.message });
  }
assert.deepEqual(differences.slice(0, 10), []);
console.log(
  `Better Auth server-only signJWT: ${inputs.filter((input) => input.op === 'sign').length} actual API and ${inputs.filter((input) => input.op === 'records').length} key-state comparisons passed`
);
