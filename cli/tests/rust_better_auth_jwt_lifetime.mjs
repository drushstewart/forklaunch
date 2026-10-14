// Actual whole-second relative expiration, safe integer boundaries and claim precedence.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const { betterAuth } = await import(pathToFileURL(path.join(dependencies, 'node_modules/better-auth/dist/index.mjs')));
const { sec } = await import(pathToFileURL(path.join(dependencies, 'node_modules/better-auth/dist/utils/time.mjs')));
for (const [text, bits] of [['9007199254740991s','433fffffffffffff'],['0.49999999999999994s','0000000000000000'],['-0.5s','8000000000000000'],['-1.5s','bff0000000000000']]) {
  const value = Buffer.alloc(8); value.writeDoubleBE(sec(text)); assert.equal(value.toString('hex'), bits);
}
let cases = 0;
for (const external of [false, true]) for (const seconds of [0, 1, 900, Number.MAX_SAFE_INTEGER]) for (const iat of [123, Number.MAX_SAFE_INTEGER, -Number.MAX_SAFE_INTEGER]) for (const override of [false, true, null]) {
  const signed = [];
  const reference = await betterAuthReference(root, { jwt: {
    ...(external ? { jwks: { remoteUrl: 'https://keys.example.test/public', keyPairConfig: { alg: 'EdDSA' } } } : {}),
    jwt: {
      expirationTime: `${seconds}s`,
      getSubject: async () => 'resolved-subject',
      definePayload: async () => ({ iat, sub: 'ignored', ...(override === false ? {} : { exp: override === null ? null : 456 }) }),
      ...(external ? { sign: async payload => { signed.push(payload); return 'owned-token'; } } : {}),
    },
  } });
  const auth = betterAuth({ ...reference.config, logger: { disabled: true } });
  const request = (route, options = {}) => auth.handler(new Request(`http://localhost:3000/api/auth${route}`, options));
  const signup = await request('/sign-up/email', { method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json' }, body: JSON.stringify({ email: 'expiration@example.test', password: 'owned-fixture-password', name: 'Expiry', firstName: 'Expiry', lastName: 'User' }) });
  assert.equal(signup.status, 200);
  const cookie = signup.headers.getSetCookie().find(value => value.startsWith('better-auth.session_token=')).split(';')[0];
  for (const route of ['/get-session', '/token']) {
    const response = await request(route, { headers: { cookie } }); assert.equal(response.status, 200);
    const body = await response.json();
    const token = route === '/token' ? body.token : response.headers.get('set-auth-jwt');
    const payload = external ? signed.at(-1) : JSON.parse(Buffer.from(token.split('.')[1], 'base64url'));
    assert.equal(payload.exp, override ? 456 : iat + seconds);
    assert.equal(payload.sub, 'resolved-subject');
    assert.equal(payload.iat, iat); assert.equal(payload.email, undefined);
    if (route === '/get-session') { assert.equal(body.user.email, 'expiration@example.test'); assert.ok(new Date(body.session.expiresAt).getTime() > Date.now()); }
    cases++;
  }
}
console.log(`Better Auth relative whole-second lifetime: ${cases} local JOSE/external route cases PASS.`);
