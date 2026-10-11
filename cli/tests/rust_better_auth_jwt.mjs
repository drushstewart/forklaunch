// Exercise native JWT issuance with the configured Better Auth payload and JOSE.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createPrivateKey } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const actual = (file) =>
  import(pathToFileURL(path.join(dependencies, 'node_modules', file)));
const [{ getJwtToken }, jose, reference] = await Promise.all([
  actual('better-auth/dist/plugins/jwt/sign.mjs'),
  actual('jose/dist/webapi/index.js'),
  betterAuthReference(root),
]);
async function native(input) {
  const child = spawn(
    'cargo',
    [
      'run',
      '--quiet',
      '--manifest-path',
      'blueprint-rs/Cargo.toml',
      '-p',
      'forklaunch-iam-better-auth',
      '--example',
      'jwt_parity',
    ],
    {
      cwd: root,
      env: { ...process.env, CARGO_INCREMENTAL: '0' },
      stdio: ['pipe', 'pipe', 'inherit'],
    },
  );
  let output = '';
  child.stdout.setEncoding('utf8').on('data', (chunk) => {
    output += chunk;
  });
  const completion = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code) =>
      code === 0
        ? resolve()
        : reject(new Error(`Native JWT fixture exited ${code}`)),
    );
  });
  child.stdin.end(JSON.stringify(input));
  await completion;
  return JSON.parse(output);
}
const issuedAt = Math.floor(Date.now() / 1000);
let checks = 0;
for (const organizationId of [null, 'organization-fixture-id']) {
  const input = {
    baseURL: reference.config.baseURL,
    userId: 'user-fixture-id',
    email: 'fixture@example.test',
    organizationId,
    issuedAt,
  };
  const result = await native(input);
  // Ring exports RFC 5958 v2. Node/OpenSSL accepts the RFC 8410 v1 form;
  // convert only this known fixture encoding, preserving the same seed.
  const encoded = Buffer.from(result.privateKey, 'base64');
  assert.equal(
    encoded.subarray(0, 16).toString('hex'),
    '3051020101300506032b657004220420',
  );
  const nodePkcs8 = Buffer.concat([
    Buffer.from('302e020100300506032b657004220420', 'hex'),
    encoded.subarray(16, 48),
  ]);
  const privateJwk = createPrivateKey({
    key: nodePkcs8,
    format: 'der',
    type: 'pkcs8',
  }).export({ format: 'jwk' });
  const publicKey = await jose.importJWK(result.jwk, 'EdDSA');
  const verification = {
    algorithms: ['EdDSA'],
    issuer: input.baseURL,
    audience: input.baseURL,
    currentDate: new Date(issuedAt * 1000),
  };
  const verified = await jose.jwtVerify(result.token, publicKey, verification);
  assert.deepEqual(verified.protectedHeader, {
    alg: 'EdDSA',
    kid: 'fixture-persistent-key-id',
  });
  assert.deepEqual(verified.payload, {
    sub: input.userId,
    email: input.email,
    activeOrganizationId: organizationId,
    iat: issuedAt,
    exp: issuedAt + 900,
    iss: input.baseURL,
    aud: input.baseURL,
  });
  assert.deepEqual(Object.keys(result.jwk).sort(), [
    'alg',
    'crv',
    'kid',
    'kty',
    'x',
  ]);
  // Only the encrypted persistence boundary is owned here. The real configured
  // definePayload and Better Auth key-selection/signing implementation execute.
  const keyRow = {
    id: result.jwk.kid,
    alg: 'EdDSA',
    crv: 'Ed25519',
    createdAt: new Date(),
    publicKey: JSON.stringify({ kty: 'OKP', crv: 'Ed25519', x: result.jwk.x }),
    privateKey: JSON.stringify(privateJwk),
  };
  const options = {
    ...reference.config.plugins.find((plugin) => plugin.id === 'jwt').options,
    jwks: { disablePrivateKeyEncryption: true },
    adapter: { getJwks: async () => [keyRow] },
  };
  const context = {
    context: {
      options: { baseURL: input.baseURL },
      session: {
        user: { id: input.userId, email: input.email, firstName: 'Excluded' },
        session: { activeOrganizationId: organizationId },
      },
    },
  };
  const originalNow = Date.now;
  let actualToken;
  try {
    Date.now = () => issuedAt * 1000;
    actualToken = await getJwtToken(context, options);
  } finally {
    Date.now = originalNow;
  }
  const actualVerified = await jose.jwtVerify(
    actualToken,
    publicKey,
    verification,
  );
  assert.deepEqual(actualVerified.protectedHeader, verified.protectedHeader);
  assert.deepEqual(actualVerified.payload, verified.payload);
  reference.database.jwks.splice(0, reference.database.jwks.length, keyRow);
  const jwks = await reference.client().call('/jwks');
  assert.equal(jwks.status, 200);
  assert.deepEqual(jwks.body, { keys: [result.jwk] });
  const restored = await native({ ...input, privateKey: result.privateKey });
  assert.equal(restored.token, result.token);
  assert.deepEqual(restored.jwk, result.jwk);
  await assert.rejects(
    jose.jwtVerify(result.token, publicKey, {
      ...verification,
      issuer: 'https://wrong.test',
    }),
  );
  await assert.rejects(
    jose.jwtVerify(result.token, publicKey, {
      ...verification,
      audience: 'https://wrong.test',
    }),
  );
  await assert.rejects(
    jose.jwtVerify(result.token, publicKey, {
      ...verification,
      currentDate: new Date((issuedAt + 900) * 1000),
    }),
  );
  const [header, payload, signature] = result.token.split('.');
  const forged = Buffer.from(signature, 'base64url');
  forged[0] ^= 1;
  await assert.rejects(
    jose.jwtVerify(
      `${header}.${payload}.${forged.toString('base64url')}`,
      publicKey,
      verification,
    ),
  );
  checks += 13;
}
console.log(`Better Auth native JWT/JWKS parity: ${checks} comparisons passed`);
