import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { actualValidator } from './rust_actual_validator_fixture.mjs';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
import { betterAuthServer } from './fixtures/rust_better_auth_server.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const actual = actualValidator(root);
const { trustedCallbackUrl, scriptJson } = actual.load(
  path.join(
    root,
    'blueprint/iam-better-auth/domain/utils/betterAuthRedirect.util.ts'
  )
);
let cases = 0;
for (const baseURL of ['http://localhost:3000', 'https://auth.example.test']) {
  const reference = await betterAuthReference(root, { baseURL });
  const context = await reference.auth.$context;
  for (const target of [
    baseURL + '/complete',
    '/complete',
    baseURL + '/complete?x=1#old'
  ]) {
    assert.equal(
      trustedCallbackUrl(target, context).origin,
      new URL(baseURL).origin
    );
    cases++;
  }
  for (const target of [
    'https://evil.example.test/',
    '//evil.example.test/',
    'javascript:alert(1)',
    'data:text/html,hello',
    baseURL.replace('://', '://user:password@') + '/complete',
    baseURL + '.evil.example.test/complete'
  ]) {
    assert.equal(trustedCallbackUrl(target, context), null, target);
    cases++;
  }
  const loaded = await betterAuthServer(root, reference, {
    onCall: () => cases++
  });
  for (const callback of [
    'https://evil.example.test/',
    'javascript:alert(1)',
    '//evil.example.test/'
  ]) {
    assert.equal(
      (await loaded.call('/api/auth/test-callback', { callbackUrl: callback }))
        .status,
      400
    );
    assert.equal(
      (await loaded.call('/api/auth/oauth-redirect', { callbackURL: callback }))
        .status,
      400
    );
  }
  assert.equal((await loaded.call('/api/auth/test-callback', {})).status, 400);
  assert.equal((await loaded.call('/api/auth/oauth-redirect', {})).status, 400);
  assert.equal(loaded.fetches(), 0);
  const client = reference.client();
  const signup = await client.call('/sign-up/email', {
    name: 'Callback User',
    firstName: 'Callback',
    lastName: 'User',
    email: 'callback@example.test',
    password: 'callback-test-only-password'
  });
  assert.equal(signup.status, 200);
  const cookies = [...client.cookies]
    .map(([name, value]) => `${name}=${value}`)
    .join('; ');
  const exchanged = await loaded.call(
    '/api/auth/test-callback',
    { callbackUrl: '/complete?x=1#old' },
    cookies
  );
  assert.equal(exchanged.status, 302);
  const location = new URL(exchanged.location),
    fragment = new URLSearchParams(location.hash.slice(1));
  assert.equal(location.origin, new URL(baseURL).origin);
  assert.equal(location.pathname, '/complete');
  assert.equal(location.search, '?x=1');
  assert.equal(fragment.get('sessionToken'), signup.body.token);
  assert.equal(fragment.get('email'), 'callback@example.test');
  assert.ok(fragment.get('token'));
  assert.equal(loaded.fetches(), 2);
  assert.ok(
    exchanged.headers['set-cookie'].startsWith(
      `${context.authCookies.sessionToken.name}=;`
    )
  );
  if (baseURL.startsWith('https:'))
    assert.match(exchanged.headers['set-cookie'], /; Secure/);
  const malicious =
    '</script><script>throw new Error("injected")</script>\u2028\u2029&';
  assert.ok(!scriptJson(malicious).includes('<'));
  assert.equal(JSON.parse(scriptJson(malicious)), malicious);
  for (const mode of ['social', 'sso']) {
    const page = await loaded.call('/api/auth/oauth-redirect', {
      provider: malicious,
      organizationId: malicious,
      callbackURL: '/complete',
      endpoint: mode
    });
    assert.equal(page.status, 200);
    assert.equal((page.body.match(/<script>/g) ?? []).length, 1);
    assert.equal((page.body.match(/<\/script>/g) ?? []).length, 1);
    const script = page.body.match(/<script>([\s\S]*)<\/script>/)[1];
    let captured;
    const window = { location: { href: '' } };
    await vm.runInNewContext(script, {
      window,
      document: { body: {} },
      fetch: async (endpoint, options) => {
        captured = { endpoint, body: JSON.parse(options.body) };
        return {
          json: async () => ({ url: 'https://accounts.google.com/mock' })
        };
      }
    });
    assert.equal(window.location.href, 'https://accounts.google.com/mock');
    assert.equal(
      captured.body[mode === 'sso' ? 'organizationId' : 'provider'],
      malicious
    );
    const intermediate = new URL(captured.body.callbackURL);
    assert.equal(intermediate.origin, new URL(baseURL).origin);
    assert.equal(
      intermediate.searchParams.get('callbackUrl'),
      baseURL + '/complete'
    );
  }
}
console.log(
  `PASS ${cases} actual Better Auth callback/helper checks: trusted origins, configured exchange authority, token hashes, secure cookie clearing and script-safe OAuth/SSO HTML`
);
