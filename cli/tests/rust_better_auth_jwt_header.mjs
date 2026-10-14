// Actual configured JWT plugin's optional header suppression. Native HTTP tests
// additionally prove that suppression avoids local key minting/external signing.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
for (const disabled of [false, true]) {
  const { auth, database } = await betterAuthReference(root, {
    jwt: { disableSettingJwtHeader: disabled },
  });
  const signup = await auth.handler(new Request('http://localhost:3000/api/auth/sign-up/email', {
    method: 'POST',
    headers: { origin: 'http://localhost:3000', 'content-type': 'application/json' },
    body: JSON.stringify({
      email: 'header@example.test', password: 'owned-fixture-password',
      name: 'Header', firstName: 'Header', lastName: 'User',
    }),
  }));
  assert.equal(signup.status, 200);
  const cookie = signup.headers.getSetCookie()
    .find(value => value.startsWith('better-auth.session_token=')).split(';')[0];
  assert.equal(database.jwks.length, 0);
  for (const method of ['GET', 'POST']) {
    const response = await auth.handler(new Request('http://localhost:3000/api/auth/get-session', {
      method, headers: { cookie, origin: 'http://localhost:3000' },
    }));
    assert.equal(response.status, method === 'GET' ? 200 : 405);
    assert.equal(response.headers.has('set-auth-jwt'), method === 'GET' && !disabled);
    assert.equal(response.headers.get('access-control-expose-headers'),
      method === 'GET' && !disabled ? 'set-auth-jwt' : null);
    if (method === 'GET') assert.equal((await response.json()).user.email, 'header@example.test');
  }
  assert.equal(database.jwks.length, disabled ? 0 : 1, 'suppression must skip key minting');
  const token = await auth.handler(new Request('http://localhost:3000/api/auth/token', { headers: { cookie } }));
  assert.equal(token.status, 200);
  assert.equal(typeof (await token.json()).token, 'string');
  const jwks = await auth.handler(new Request('http://localhost:3000/api/auth/jwks'));
  assert.equal(jwks.status, 200);
  assert.equal((await jwks.json()).keys.length, 1);
}
console.log('Better Auth JWT header: optional suppression, unchanged explicit token/JWKS and skipped key minting PASS.');
