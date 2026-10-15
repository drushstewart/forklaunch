import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../..'
);
const prefix =
  process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/';
const { betterAuthReference } = await import(
  pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs')
);
const { betterAuth } = await import(
  pathToFileURL(prefix + 'better-auth/dist/index.mjs')
);
const { twoFactor } = await import(
  pathToFileURL(prefix + 'better-auth/dist/plugins/two-factor/index.mjs')
);
const { symmetricDecrypt } = await import(
  pathToFileURL(prefix + 'better-auth/dist/crypto/index.mjs')
);
const { createOTP } = await import(
  pathToFileURL(prefix + '@better-auth/utils/dist/otp.mjs')
);
const ref = await betterAuthReference(root);
ref.database.twoFactor = [];
let auth;
const jar = new Map();
const configure = (opts) =>
  (auth = betterAuth({
    ...ref.config,
    plugins: [...ref.config.plugins, twoFactor(opts)],
    logger: { disabled: true },
    rateLimit: { enabled: false }
  }));
async function call(route, body = {}) {
  const r = await auth.handler(
    new Request('http://localhost:3000/api/auth' + route, {
      method: 'POST',
      headers: {
        origin: 'http://localhost:3000',
        'content-type': 'application/json',
        cookie: [...jar].map(([k, v]) => k + '=' + v).join('; ')
      },
      body: JSON.stringify(body)
    })
  );
  for (const c of r.headers.getSetCookie()) {
    const [pair] = c.split(';');
    const i = pair.indexOf('=');
    if (c.includes('Max-Age=0')) jar.delete(pair.slice(0, i));
    else jar.set(pair.slice(0, i), pair.slice(i + 1));
  }
  return {
    status: r.status,
    body: await r.json(),
    cookies: r.headers.getSetCookie().length
  };
}
const credentials = {
  name: 'User',
  firstName: 'User',
  lastName: 'Name',
  email: 'life@example.test',
  password: 'owned-factor-password'
};
configure({
  skipVerificationOnEnable: true,
  issuer: 'Enrollment issuer',
  totpOptions: { issuer: 'URI issuer' }
});
await call('/sign-up/email', credentials);
const original = ref.database.session[0].token;
await call('/sign-in/email', credentials);
assert.equal(ref.database.session.length, 2);
const before = new Set(ref.database.session.map((s) => s.token));
const enrollment = await call('/two-factor/enable', {
  password: credentials.password,
  issuer: ''
});
assert.equal(
  new URL(enrollment.body.totpURI).searchParams.get('issuer'),
  'Enrollment issuer'
);
const configuredUri = await call('/two-factor/get-totp-uri', {
  password: credentials.password
});
assert.equal(
  new URL(configuredUri.body.totpURI).searchParams.get('issuer'),
  'URI issuer'
);
assert.equal(enrollment.status, 200);
assert.equal(ref.database.twoFactor[0].verified, true);
assert.equal(ref.database.user[0].twoFactorEnabled, true);
assert.equal(ref.database.session.length, 2);
assert.ok(ref.database.session.some((s) => s.token === original));
assert.equal(ref.database.session.filter((s) => before.has(s.token)).length, 1);
assert.equal(ref.database.verification.length, 0);
console.log(
  'PASS skip enrollment: enables factor and flag, rotates current only, preserves other session, no trust/challenge'
);
await call('/sign-out');
assert.equal(
  (await call('/sign-in/email', credentials)).body.twoFactorRedirect,
  true
);
const ctx = await auth.$context;
const secret = await symmetricDecrypt({
  key: ctx.secretConfig,
  data: ref.database.twoFactor[0].secret
});
const code = await createOTP(secret).totp();
assert.equal(
  (await call('/two-factor/verify-totp', { code, trustDevice: true })).status,
  200
);
assert.ok(jar.has('better-auth.trust_device'));
await call('/sign-out');
configure({ totpOptions: { disable: true } });
const trusted = await call('/sign-in/email', credentials);
assert.equal(trusted.status, 200);
assert.equal(trusted.body.twoFactorRedirect, undefined);
assert.ok(trusted.body.token);
assert.ok(jar.has('better-auth.trust_device'));
await call('/sign-out');
jar.delete('better-auth.trust_device');
const pending = await call('/sign-in/email', credentials);
assert.deepEqual(pending.body, {
  twoFactorRedirect: true,
  twoFactorMethods: []
});
assert.equal(
  (await call('/two-factor/verify-totp', { code })).body.code,
  'TOTP_NOT_CONFIGURED'
);
assert.equal(
  (
    await call('/two-factor/verify-backup-code', {
      code: enrollment.body.backupCodes[0]
    })
  ).status,
  200
);
console.log(
  'PASS disabled TOTP: trust remains valid; no-trust empty-method challenge permits backup, no TOTP'
);
// Global passwordless permission cannot bypass a credential password.
configure({
  allowPasswordless: true,
  totpOptions: { allowPasswordless: false },
  backupCodeOptions: { allowPasswordless: false }
});
assert.equal((await call('/two-factor/disable')).body.code, 'INVALID_PASSWORD');
const credentialTemplate = structuredClone(ref.database.account[0]);
ref.database.account = [];
assert.equal((await call('/two-factor/get-totp-uri')).status, 400);
assert.equal((await call('/two-factor/generate-backup-codes')).status, 400);
assert.equal((await call('/two-factor/disable')).status, 200);
assert.equal(ref.database.twoFactor.length, 0);
assert.equal(ref.database.user[0].twoFactorEnabled, false);
assert.equal((await call('/two-factor/enable')).status, 200);
configure({ allowPasswordless: true });
assert.equal((await call('/two-factor/get-totp-uri')).status, 200);
const enabledCode = await createOTP(
  await symmetricDecrypt({
    key: (await auth.$context).secretConfig,
    data: ref.database.twoFactor[0].secret
  })
).totp();
await call('/two-factor/verify-totp', { code: enabledCode });
assert.equal((await call('/two-factor/generate-backup-codes')).status, 200);
console.log(
  'PASS passwordless: credential still required; absent credential permits global operations; nested false retains URI/backup requirement; inherited true enables them'
);

for (const password of ['', null]) {
  ref.database.account = [{ ...credentialTemplate, password }];
  assert.equal((await call('/two-factor/get-totp-uri')).status, 200);
}
ref.database.account = [
  { ...credentialTemplate, password: 'nonempty-credential' }
];
assert.equal(
  (await call('/two-factor/get-totp-uri')).body.code,
  'INVALID_PASSWORD'
);
console.log(
  'PASS passwordless credential truthiness: empty/null absent, nonempty requires password'
);
ref.database.account = [];
configure({ allowPasswordless: false });
assert.equal(
  (await call('/two-factor/get-totp-uri', { password: 'x'.repeat(129) })).body
    .code,
  'PASSWORD_TOO_LONG'
);
configure({ allowPasswordless: true });
assert.equal(
  (await call('/two-factor/get-totp-uri', { password: 'x'.repeat(129) }))
    .status,
  200
);
console.log(
  'PASS absent credential: required-password length check precedes account lookup; passwordless ignores unused password'
);
