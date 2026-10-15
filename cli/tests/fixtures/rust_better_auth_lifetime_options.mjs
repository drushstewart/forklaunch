import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const { betterAuthReference } = await import(pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const prefix = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/better-auth/dist/';
const { betterAuth } = await import(pathToFileURL(prefix + 'index.mjs'));
const { twoFactor } = await import(pathToFileURL(prefix + 'plugins/two-factor/index.mjs'));
const RealDate = Date;
let clock = 1800000000000;
globalThis.Date = class extends RealDate {
  constructor(...args) { super(...(args.length ? args : [clock])); }
  static now() { return clock; }
  static [Symbol.hasInstance](x) { return x instanceof RealDate; }
};
const observations = [];
try {
  for (const challengeSeconds of [undefined, 0, 2]) {
    for (const offset of [-1, 0, 1]) {
      const ref = await betterAuthReference(root);
      ref.database.twoFactor = [];
      let auth;
      const jar = new Map();
      const initialTrust = challengeSeconds === undefined ? undefined : challengeSeconds === 0 ? 0 : 3;
      function configure(trustSeconds = initialTrust) {
        auth = betterAuth({ ...ref.config, plugins: [...ref.config.plugins, twoFactor({ skipVerificationOnEnable: true, twoFactorCookieMaxAge: challengeSeconds, trustDeviceMaxAge: trustSeconds })], logger: { disabled: true }, rateLimit: { enabled: false } });
      }
      async function call(route, body = {}) {
        const response = await auth.handler(new Request('http://localhost:3000/api/auth' + route, { method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json', cookie: [...jar].map(([k,v]) => k + '=' + v).join('; ') }, body: JSON.stringify(body) }));
        const cookies = response.headers.getSetCookie();
        // Deliberately retain Max-Age=0 challenge values to probe server authority
        // independently of browser expiry. Empty deletion values are removed.
        for (const cookie of cookies) {
          const pair = cookie.split(';')[0]; const at = pair.indexOf('=');
          if (pair.slice(at + 1)) jar.set(pair.slice(0, at), pair.slice(at + 1)); else jar.delete(pair.slice(0, at));
        }
        return { status: response.status, body: await response.json(), cookies };
      }
      configure();
      const credentials = { name: 'TTL', firstName: 'TTL', lastName: 'Test', email: 'ttl@example.test', password: 'owned-factor-password' };
      assert.equal((await call('/sign-up/email', credentials)).status, 200);
      const enrolled = await call('/two-factor/enable', { password: credentials.password });
      assert.equal(enrolled.status, 200);
      await call('/sign-out');
      const signin = await call('/sign-in/email', credentials);
      assert.equal(signin.body.twoFactorRedirect, true);
      const seconds = challengeSeconds ?? 600;
      assert.ok(signin.cookies.some(c => c.startsWith('better-auth.two_factor=') && c.includes('Max-Age=' + seconds)));
      const row = ref.database.verification.find(r => r.identifier.startsWith('2fa-') && !r.identifier.startsWith('2fa-attempts-'));
      assert.equal(+row.expiresAt, clock + seconds * 1000);
      clock = +row.expiresAt + offset;
      const verified = await call('/two-factor/verify-backup-code', { code: enrolled.body.backupCodes[0], trustDevice: true });
      observations.push({ challengeSeconds: seconds, offset, status: verified.status, code: verified.body.code });
      assert.equal(verified.status, offset <= 0 ? 200 : 401);
      if (verified.status === 200) {
        const trust = ref.database.verification.find(r => r.identifier.startsWith('trust-device-'));
        assert.equal(+trust.expiresAt, clock + (initialTrust ?? 2592000) * 1000);
        assert.ok(verified.cookies.some(c => c.startsWith('better-auth.trust_device=') && c.includes('Max-Age=' + (initialTrust ?? 2592000))));
        await call('/sign-out');
        configure(7);
        clock = +trust.expiresAt + offset;
        const renewed = await call('/sign-in/email', credentials);
        assert.equal(renewed.body.twoFactorRedirect === true, offset >= 0);
        if (offset < 0) {
          const next = ref.database.verification.find(r => r.identifier.startsWith('trust-device-'));
          assert.notEqual(next.identifier, trust.identifier);
          assert.equal(+next.expiresAt, clock + 7000);
          assert.ok(renewed.cookies.some(c => c.startsWith('better-auth.trust_device=') && c.includes('Max-Age=7')));
        }
      }
      clock += 10000;
    }
  }
  console.log('PASS actual lifetime options: nine challenge boundary cases, strict trust expiry, changed-policy renewal and cookie/record TTLs', JSON.stringify(observations));
} finally { globalThis.Date = RealDate; }
