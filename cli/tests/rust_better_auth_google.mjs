// Real Google ID-token verification/handlers with only Google's JWKS HTTP owned.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const jose = await import(
  pathToFileURL(
    path.join(dependencies, 'node_modules/jose/dist/webapi/index.js'),
  )
);
const reference = await betterAuthReference(root, { google: true });
const { publicKey, privateKey } = await jose.generateKeyPair('RS256', {
  extractable: true,
});
const publicJwk = {
  ...(await jose.exportJWK(publicKey)),
  kid: 'fixture-google-key',
  alg: 'RS256',
  use: 'sig',
};
const originalFetch = globalThis.fetch;
let keyRequests = 0;
globalThis.fetch = async (input) => {
  const url =
    typeof input === 'string'
      ? input
      : input instanceof URL
        ? input.href
        : input.url;
  assert.equal(
    url,
    'https://www.googleapis.com/oauth2/v3/certs',
    'unexpected outbound request',
  );
  keyRequests++;
  return new Response(JSON.stringify({ keys: [publicJwk] }), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
};
const now = Math.floor(Date.now() / 1000);
const profile = {
  sub: 'fixture-google-subject',
  email: 'google@example.test',
  email_verified: true,
  name: 'Google Person',
  given_name: 'Google',
  family_name: 'Person',
  picture: 'https://fixture.test/avatar.png',
  nonce: 'fixture-google-nonce',
};
async function token(changes = {}, header = {}) {
  return new jose.SignJWT({
    ...profile,
    iat: now,
    exp: now + 300,
    iss: 'https://accounts.google.com',
    aud: 'fixture-google-id',
    ...changes,
  })
    .setProtectedHeader({ alg: 'RS256', kid: publicJwk.kid, ...header })
    .sign(privateKey);
}
let checks = 0;
async function signIn(who, signed, nonce = profile.nonce, status = 200) {
  const response = await who.call('/sign-in/social', {
    provider: 'google',
    idToken: { token: signed, nonce },
  });
  assert.equal(response.status, status, JSON.stringify(response.body));
  if (status === 401) assert.equal(response.body.code, 'INVALID_TOKEN');
  checks++;
  return response;
}
try {
  const who = reference.client();
  const valid = await token();
  const response = await signIn(who, valid);
  assert.equal(response.body.redirect, false);
  assert.equal(response.body.user.firstName, 'Google');
  assert.equal(response.body.user.lastName, 'Person');
  assert.equal(response.body.user.emailVerified, true);
  assert.ok(response.body.token);
  assert.equal(reference.database.account.length, 1);
  assert.equal(reference.database.account[0].accountId, profile.sub);
  assert.equal(reference.database.account[0].providerId, 'google');
  assert.equal(reference.database.account[0].idToken, valid);
  const session = await who.call('/get-session');
  assert.equal(session.body.user.id, response.body.user.id);
  await signIn(reference.client(), valid);
  assert.equal(reference.database.user.length, 1);
  assert.equal(reference.database.account.length, 1);
  checks += 12;
  for (const changes of [
    { aud: 'wrong-client' },
    { iss: 'https://untrusted.test' },
    { exp: now - 60 },
    { iat: now - 3601 },
    { iat: now + 60 },
  ])
    await signIn(reference.client(), await token(changes), profile.nonce, 401);
  await signIn(reference.client(), valid, 'wrong-nonce', 401);
  await signIn(
    reference.client(),
    await token({}, { kid: 'missing-key' }),
    profile.nonce,
    401,
  );
  const parts = valid.split('.');
  const forged = Buffer.from(parts[2], 'base64url');
  forged[0] ^= 1;
  parts[2] = forged.toString('base64url');
  await signIn(reference.client(), parts.join('.'), profile.nonce, 401);
  assert.equal(reference.database.user.length, 1);
  const missingNames = await token({
    sub: 'fixture-mononym-subject',
    email: 'mononym@example.test',
    given_name: undefined,
    family_name: undefined,
    name: 'Mononym',
  });
  const mononym = await signIn(reference.client(), missingNames);
  assert.equal(mononym.body.user.firstName, '');
  assert.equal(mononym.body.user.lastName, '');
  const malformedNames = await token({
    sub: 'fixture-malformed-subject',
    email: 'malformed@example.test',
    given_name: 42,
    family_name: ['bad'],
  });
  const malformed = await signIn(reference.client(), malformedNames);
  assert.equal(malformed.body.user.firstName, '');
  assert.equal(malformed.body.user.lastName, '');
  checks += 5;
  assert.ok(keyRequests >= 12);
  console.log(
    `Configured Better Auth Google protocol: ${checks} checks passed`,
  );
} finally {
  globalThis.fetch = originalFetch;
}
