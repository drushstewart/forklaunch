// Execute the actual TS server helpers through Express and compare native wire
// results. Only the auth exchange transport, storage and frozen clock are owned.
import assert from 'node:assert/strict';
import { createPublicKey, verify } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
import { betterAuthServer } from './fixtures/rust_better_auth_server.mjs';
import { nativeAuthBridge } from './fixtures/rust_better_auth_bridge.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const dependencies = process.env.FORKLAUNCH_TEST_EXPRESS_DEPENDENCIES;
assert.ok(
  dependencies,
  'Set FORKLAUNCH_TEST_EXPRESS_DEPENDENCIES to Express 5.2.1'
);
const { default: express } = await import(
  pathToFileURL(path.join(dependencies, 'node_modules/express/index.js'))
);
const RealDate = Date;
const now = Math.floor(Date.now() / 1000) * 1000;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [now]));
  }
  static now() {
    return now;
  }
};
let checks = 0;
try {
  for (const { baseURL, basePath } of [
    { baseURL: 'http://localhost:3000', basePath: '/api/auth' },
    { baseURL: 'https://auth.example.test', basePath: '/api/auth' },
    { baseURL: 'http://localhost:3000', basePath: '/identity/login' },
    { baseURL: 'https://auth.example.test', basePath: 'identity/login' },
    { baseURL: 'https://auth.example.test/custom/auth', basePath: '/ignored' },
    { baseURL: 'http://localhost:3000/custom/auth/', basePath: '/ignored' },
    { baseURL: 'https://auth.example.test/', basePath: '/login/' },
    { baseURL: 'http://localhost:3000', basePath: '/' },
    { baseURL: 'https://auth.example.test', basePath: '' }
  ]) {
    const reference = await betterAuthReference(root, {
      baseURL,
      basePath,
      google: true
    });
    const authContext = await reference.auth.$context;
    const effectivePath = new URL(authContext.baseURL).pathname.replace(
      /\/+$/,
      ''
    );
    const loaded = await betterAuthServer(root, reference);
    const app = express();
    for (const [route, handler] of loaded.routes)
      app.get(route, (request, response, next) => {
        Promise.resolve(handler(request, response)).catch(next);
      });
    app.all(loaded.rawPaths, (_request, response) =>
      response.status(204).end()
    );
    app.get('/discovery/auth-methods', (_request, response) =>
      response.json({ application: true })
    );
    const server = await new Promise((resolve) => {
      const server = app.listen(0, '127.0.0.1', () => resolve(server));
    });
    const bridge = nativeAuthBridge(root, {
      features: 'application-http',
      env: {
        FORKLAUNCH_AUTH_POPUP_HELPERS: 'true',
        FORKLAUNCH_AUTH_BASE_URL: baseURL,
        FORKLAUNCH_AUTH_BASE_PATH: basePath,
        FORKLAUNCH_GOOGLE_CERTIFICATES_URL: 'http://127.0.0.1:1/certs',
        NODE_ENV: 'development'
      }
    });
    const native = (input) => bridge.native({ now, headers: {}, ...input });
    async function compare(
      route,
      search,
      headers = {},
      method = 'GET',
      signed = false
    ) {
      const expected = await fetch(
        `http://127.0.0.1:${server.address().port}${effectivePath}/${route}${search}`,
        {
          method,
          headers,
          redirect: 'manual',
          signal: AbortSignal.timeout(10000)
        }
      );
      const actual = await native({
        op: 'request',
        method,
        path: `/${route}${search}`,
        headers
      });
      assert.equal(
        actual.status,
        expected.status,
        `${route}${search} accept=${headers.accept}`
      );
      let expectedBody = await expected.text();
      let actualBody = actual.body;
      let actualToken, expectedToken;
      if (signed) {
        actualToken = new URLSearchParams(
          new URL(actual.headers.location[0]).hash.slice(1)
        ).get('token');
        expectedToken = new URLSearchParams(
          new URL(expected.headers.get('location')).hash.slice(1)
        ).get('token');
        const actualKeys = (
          await native({
            op: 'request',
            method: 'GET',
            path: '/jwks',
            headers: {}
          })
        ).body;
        const expectedKeys = (await reference.client().call('/jwks')).body;
        function claims(token, keys) {
          const [header, payload, signature] = token.split('.');
          const kid = JSON.parse(Buffer.from(header, 'base64url')).kid;
          const key = keys.keys.find((key) => key.kid === kid);
          assert.ok(
            verify(
              null,
              Buffer.from(`${header}.${payload}`),
              createPublicKey({ key, format: 'jwk' }),
              Buffer.from(signature, 'base64url')
            )
          );
          return JSON.parse(Buffer.from(payload, 'base64url'));
        }
        assert.deepEqual(
          claims(actualToken, actualKeys),
          claims(expectedToken, expectedKeys)
        );
        actualBody = actualBody.replace(actualToken, '<signed-jwt>');
        expectedBody = expectedBody.replace(expectedToken, '<signed-jwt>');
      }
      assert.equal(
        actualBody,
        expectedBody,
        `${route}${search} accept=${headers.accept}`
      );
      for (const name of ['content-type', 'location', 'set-cookie', 'vary']) {
        const actualHeader = actual.headers[name]?.[0] ?? null;
        const expectedHeader = expected.headers.get(name);
        assert.equal(
          signed && name === 'location'
            ? actualHeader.replace(actualToken, '<signed-jwt>')
            : actualHeader,
          signed && name === 'location'
            ? expectedHeader.replace(expectedToken, '<signed-jwt>')
            : expectedHeader,
          `${route}${search} ${name}`
        );
      }
      checks++;
      return actual;
    }
    try {
      const transport = `http://127.0.0.1:${server.address().port}`;
      for (const endpoint of reference.routes) {
        const probe = endpoint.path.replace(/:[A-Za-z]+/g, 'fixture');
        const response = await fetch(`${transport}${effectivePath}${probe}`, {
          redirect: 'manual'
        });
        assert.equal(
          response.status,
          204,
          `${basePath} raw dispatch: ${probe}`
        );
      }
      assert.deepEqual(
        await (await fetch(`${transport}/discovery/auth-methods`)).json(),
        { application: true }
      );
      checks++;
      const schema = await native({
        op: 'request',
        method: 'GET',
        path: '/open-api/generate-schema'
      });
      assert.equal(schema.status, 200);
      assert.deepEqual(
        schema.body,
        JSON.parse(
          JSON.stringify(await reference.auth.api.generateOpenAPISchema())
        )
      );
      checks++;
      const expected = await reference
        .client()
        .call('/sign-in/social', { provider: 'google', disableRedirect: true });
      const actual = await native({
        op: 'request',
        method: 'POST',
        path: '/sign-in/social',
        headers: {
          origin: new URL(baseURL).origin,
          'content-type': 'application/json'
        },
        body: JSON.stringify({ provider: 'google', disableRedirect: true })
      });
      assert.equal(expected.status, 200);
      assert.equal(actual.status, 200);
      const nativeURL = new URL(actual.body.url),
        referenceURL = new URL(expected.body.url);
      for (const key of ['state', 'code_challenge']) {
        assert.ok(nativeURL.searchParams.get(key));
        assert.ok(referenceURL.searchParams.get(key));
        nativeURL.searchParams.delete(key);
        referenceURL.searchParams.delete(key);
      }
      assert.equal(nativeURL.href, referenceURL.href);
      assert.equal(
        nativeURL.searchParams.get('redirect_uri'),
        `${authContext.baseURL}/callback/google`
      );
      checks++;
      const reset = await reference
        .client()
        .call('/reset-password/missing?callbackURL=', undefined, {
          method: 'GET'
        });
      const nativeReset = await native({
        op: 'request',
        method: 'GET',
        path: '/reset-password/missing?callbackURL='
      });
      assert.equal(nativeReset.status, reset.status);
      assert.equal(
        nativeReset.headers.location?.[0] ?? null,
        reset.headers.get('location')
      );
      checks++;
      for (const route of ['test-callback', 'oauth-redirect']) {
        const key = route === 'test-callback' ? 'callbackUrl' : 'callbackURL';
        await compare(route, '');
        for (const value of [
          '',
          'https://evil.example.test/',
          '//evil.example.test/',
          'javascript:alert(1)',
          'data:text/html,hello',
          'ftp://localhost/file',
          baseURL.replace('://', '://user:password@') + '/complete'
        ]) {
          await compare(route, `?${key}=${encodeURIComponent(value)}`);
        }
      }
      for (const accept of [
        undefined,
        '',
        '*/*',
        'text/html',
        'text/plain',
        'application/json',
        'text/*',
        'text/html;q=0.5,text/plain;q=0.9',
        'text/html;q=0.9,text/plain;q=0.5',
        '*/*;q=1,text/plain;q=0',
        'text/html;q=0,text/plain;q=0',
        'text/plain;q=0.5,text/html;q=0.5',
        'text/html;q=0.5,text/plain;q=0.5',
        'TEXT/HTML',
        'text/html;charset=utf-8',
        'text/html;foo=*',
        'text/html;q=0.5;foo=x'
      ]) {
        await compare(
          'test-callback',
          '?callbackUrl=%2Fcomplete%3Fx%3D1%26y%3D2%23old',
          accept !== undefined ? { accept } : {}
        );
      }
      await compare('test-callback', '?callbackUrl=%2Fcomplete', {
        cookie: 'better-auth.session_token=forged',
        host: 'evil.example.test'
      });
      await compare('test-callback', '?callbackUrl=%2Fa&callbackUrl=%2Fb');
      await compare('test-callback', '?callbackUrl=%20');
      const malicious =
        '</script><script>throw new Error("injected")</script>\u2028\u2029&';
      for (const endpoint of ['social', 'sso', 'sso&endpoint=social']) {
        const search = `?callbackURL=%2Fcomplete%3Fx%3D1%23old&provider=${encodeURIComponent(malicious)}&organizationId=${encodeURIComponent(malicious)}&endpoint=${endpoint}`;
        await compare('oauth-redirect', search);
      }
      await compare(
        'oauth-redirect',
        '?callbackURL=%2Fa&provider=one&provider=two'
      );
      await compare('oauth-redirect', '?callbackURL=%2Fcomplete', {}, 'HEAD');
      await compare('test-callback', '?callbackUrl=%2Fcomplete', {}, 'HEAD');
      const client = reference.client();
      const signup = await client.call('/sign-up/email', {
        email: 'popup@example.test',
        password: 'popup-fixture-password',
        name: 'Popup User',
        firstName: 'Popup',
        lastName: 'User'
      });
      assert.equal(signup.status, 200);
      assert.equal((await client.call('/get-session')).status, 200);
      const database = reference.database;
      await native({
        op: 'seed',
        users: database.user,
        accounts: database.account,
        sessions: database.session
      });
      const cookie = [...client.cookies]
        .map(([name, value]) => `${name}=${value}`)
        .join('; ');
      const before = await native({
        op: 'request',
        method: 'GET',
        path: '/get-session',
        headers: { cookie }
      });
      assert.equal(
        before.body?.user?.id,
        signup.body.user.id,
        JSON.stringify(before)
      );
      const exchanged = await compare(
        'test-callback',
        '?callbackUrl=%2Fcomplete%3Fx%3D1%23old',
        { cookie, host: 'evil.example.test', accept: 'text/html' },
        'GET',
        true
      );
      const fragment = new URLSearchParams(
        new URL(exchanged.headers.location[0]).hash.slice(1)
      );
      assert.equal(fragment.get('sessionToken'), signup.body.token);
      assert.equal(fragment.get('email'), 'popup@example.test');
      assert.ok(fragment.get('token'));
      const retained = await native({
        op: 'request',
        method: 'GET',
        path: '/get-session',
        headers: { cookie }
      });
      assert.equal(retained.body.user.id, signup.body.user.id);
      checks++;
      // Both real handlers fail closed when the selected private key is corrupt.
      const corrupt = {
        id: 'popup-corrupt',
        alg: 'EdDSA',
        createdAt: new RealDate(now + 1).toISOString(),
        privateKey: 'invalid',
        publicKey: '{}'
      };
      reference.database.jwks.splice(0, Infinity, {
        ...corrupt,
        createdAt: new RealDate(now + 1)
      });
      await native({ op: 'seed-keys', keys: [corrupt] });
      await compare('test-callback', '?callbackUrl=%2Fcomplete', { cookie });
      for (const session of reference.database.session)
        session.expiresAt = new RealDate(now - 1);
      await native({
        op: 'replace-seed',
        users: database.user,
        accounts: database.account,
        sessions: database.session
      });
      await compare('test-callback', '?callbackUrl=%2Fcomplete', { cookie });
    } finally {
      await bridge.close();
      await new Promise((resolve) => server.close(resolve));
    }
  }
} finally {
  globalThis.Date = RealDate;
}
console.log(
  `PASS ${checks} actual Express/TS base-path, OpenAPI, Google/reset callback and popup helper wire comparisons: origins, token/session exchange, secure cookie clearing, script escaping, duplicate queries, Accept negotiation and HEAD`
);
