// Actual optional getSubject callback, independently of the configured signer.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const { betterAuth } = await import(pathToFileURL(path.join(dependencies, 'node_modules/better-auth/dist/index.mjs')));
let cases = 0;
for (const external of [false, true]) {
  for (const mode of ['custom', 'empty', 'undefined', 'null', 'throw', 'disabled']) {
    const calls = [], signed = [], errors = [];
    const reference = await betterAuthReference(root, { jwt: {
      disableSettingJwtHeader: mode === 'disabled',
      ...(external ? { jwks: { remoteUrl: 'https://keys.example.test/public', keyPairConfig: { alg: 'EdDSA' } } } : {}),
      jwt: {
        getSubject: async snapshot => {
          calls.push(snapshot);
          if (mode === 'throw') throw new Error('owned private subject failure');
          return mode === 'empty' ? '' : mode === 'undefined' ? undefined : mode === 'null' ? null : `subject:${snapshot.user.id}`;
        },
        ...(external ? { sign: async payload => { signed.push(payload); return 'owned-signed-token'; } } : {}),
      },
    } });
    const auth = betterAuth({ ...reference.config, logger: { disabled: true }, onAPIError: { onError: error => errors.push(error) } });
    const request = (route, options = {}) => auth.handler(new Request(`http://localhost:3000/api/auth${route}`, options));
    assert.equal((await request('/token')).status, 401);
    const anonymous = await request('/get-session');
    assert.equal(anonymous.status, 200);
    assert.equal(await anonymous.json(), null);
    assert.equal(calls.length, 0);
    const signup = await request('/sign-up/email', {
      method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json' },
      body: JSON.stringify({ email: 'subject@example.test', password: 'owned-fixture-password', name: 'Subject', firstName: 'Subject', lastName: 'User' }),
    });
    assert.equal(signup.status, 200);
    const cookie = signup.headers.getSetCookie().find(value => value.startsWith('better-auth.session_token=')).split(';')[0];
    for (const route of ['/get-session', '/token']) {
      const response = await request(route, { headers: { cookie } });
      assert.equal(response.status, mode === 'throw' ? 500 : 200);
      if (mode === 'throw') {
        assert.equal(await response.text(), '');
        assert.deepEqual(response.headers.getSetCookie(), []);
        assert.equal(reference.database.jwks.length, 0);
        assert.equal(signed.length, 0);
      } else if (mode === 'disabled' && route === '/get-session') {
        assert.equal(response.headers.get('set-auth-jwt'), null);
        assert.equal(calls.length, 0);
        assert.equal(reference.database.jwks.length, 0);
      } else {
        const body = await response.json();
        const token = route === '/token' ? body.token : response.headers.get('set-auth-jwt');
        const payload = external ? signed.at(-1) : JSON.parse(Buffer.from(token.split('.')[1], 'base64url'));
        const snapshot = calls.at(-1);
        assert.equal(snapshot.user.id, snapshot.session.userId);
        assert.equal(payload.sub, mode === 'empty' ? '' : ['undefined', 'null'].includes(mode) ? snapshot.user.id : `subject:${snapshot.user.id}`);
        assert.equal(payload.sessionId, snapshot.session.id);
        assert.equal(payload.email, snapshot.user.email);
        if (route === '/get-session') assert.equal(body.user.id, snapshot.user.id);
      }
      cases++;
    }
    assert.equal(calls.length, mode === 'disabled' ? 1 : 2);
    assert.equal(errors.length, mode === 'throw' ? 2 : 0);
    if (external) assert.equal(reference.database.jwks.length, 0);
  }
}
console.log(`Better Auth subject hook: ${cases} local/external route cases PASS.`);
