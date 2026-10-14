// Actual optional definePayload callback: claim precedence, JOSE and external defaults.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const { betterAuth } = await import(pathToFileURL(path.join(dependencies, 'node_modules/better-auth/dist/index.mjs')));
let cases = 0;
const modes = {
  custom: { custom: true, sub: 'ignored', iat: 123, exp: 456, iss: 'issuer', aud: ['audience'] },
  empty: {}, none: undefined, null: null,
  nullClaims: { iat: null, exp: null, iss: null, aud: null },
  invalidIat: { iat: {}, exp: 456 }, invalidExp: { exp: {} },
  invalidIss: { iss: {} }, invalidAud: { aud: [42] },
  throw: {}, disabled: {},
};
for (const external of [false, true]) for (const [mode, claims] of Object.entries(modes)) {
  const order = [], signed = [], errors = [];
  const reference = await betterAuthReference(root, { jwt: {
    disableSettingJwtHeader: mode === 'disabled',
    ...(external ? { jwks: { remoteUrl: 'https://keys.example.test/public', keyPairConfig: { alg: 'EdDSA' } } } : {}),
    jwt: {
      definePayload: async snapshot => {
        assert.equal(snapshot.user.id, snapshot.session.userId); order.push('payload');
        if (mode === 'throw') throw Error('private payload fixture failure');
        return claims;
      },
      getSubject: async () => { order.push('subject'); return 'resolved-subject'; },
      ...(external ? { sign: async payload => { order.push('sign'); signed.push(payload); return 'owned-token'; } } : {}),
    },
  } });
  const auth = betterAuth({ ...reference.config, logger: { disabled: true }, onAPIError: { onError: error => errors.push(error) } });
  const request = (route, options = {}) => auth.handler(new Request(`http://localhost:3000/api/auth${route}`, options));
  assert.equal((await request('/token')).status, 401);
  assert.equal(await (await request('/get-session')).json(), null); assert.equal(order.length, 0);
  const signup = await request('/sign-up/email', { method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json' }, body: JSON.stringify({ email: 'payload@example.test', password: 'owned-fixture-password', name: 'Payload', firstName: 'Payload', lastName: 'User' }) });
  assert.equal(signup.status, 200);
  const cookie = signup.headers.getSetCookie().find(value => value.startsWith('better-auth.session_token=')).split(';')[0];
  for (const route of ['/get-session', '/token']) {
    order.length = 0;
    const response = await request(route, { headers: { cookie } });
    const failed = mode === 'throw' || (!external && mode.startsWith('invalid'));
    assert.equal(response.status, failed ? 500 : 200, `${external}/${mode}/${route}`);
    if (failed) {
      assert.equal(await response.text(), ''); assert.deepEqual(response.headers.getSetCookie(), []);
      assert.deepEqual(order, mode === 'throw' ? ['payload'] : ['payload', 'subject']);
      assert.equal(reference.database.jwks.length, mode === 'throw' ? 0 : 1);
    } else if (mode === 'disabled' && route === '/get-session') {
      assert.equal(response.headers.get('set-auth-jwt'), null); assert.deepEqual(order, []); assert.equal(reference.database.jwks.length, 0);
    } else {
      const token = route === '/token' ? (await response.json()).token : response.headers.get('set-auth-jwt');
      const payload = external ? signed.at(-1) : JSON.parse(Buffer.from(token.split('.')[1], 'base64url'));
      assert.deepEqual(order, external ? ['payload', 'subject', 'sign'] : ['payload', 'subject']);
      assert.equal(payload.sub, 'resolved-subject'); assert.equal(payload.email, undefined); assert.equal(payload.sessionId, undefined);
      if (mode === 'custom') for (const field of ['iat', 'exp', 'iss', 'aud', 'custom']) assert.deepEqual(payload[field], claims[field]);
      if (mode === 'nullClaims') { assert.equal(payload.iat, null); assert.equal(payload.iss, 'http://localhost:3000'); assert.equal(typeof payload.exp, 'number'); }
      if (external) { assert.equal(reference.database.jwks.length, 0); if (mode.startsWith('invalid')) for (const [key, value] of Object.entries(claims)) assert.deepEqual(payload[key], value); }
    }
    cases++;
  }
}
console.log(`Better Auth payload hook: ${cases} actual local JOSE/external route cases PASS.`);
