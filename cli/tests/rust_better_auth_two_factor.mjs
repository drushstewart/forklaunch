import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
assert.ok(
  process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES,
  'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES'
);
const prefix =
  process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/';
const load = (f) => import(pathToFileURL(prefix + f));
const { betterAuthReference } = await import(
  pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs')
);
const { betterAuth } = await load('better-auth/dist/index.mjs');
const { twoFactor } = await load(
  'better-auth/dist/plugins/two-factor/index.mjs'
);
const { symmetricDecrypt } = await load('better-auth/dist/crypto/index.mjs');
const { createOTP } = await load('@better-auth/utils/dist/otp.mjs');
const RealDate = Date;
let clock = 1800000000000;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [clock]));
  }
  static now() {
    return clock;
  }
};
const reference = await betterAuthReference(root);
reference.database.twoFactor = [];
const plugin = twoFactor();
assert.equal(plugin.rateLimit[0].window, 10);
assert.equal(plugin.rateLimit[0].max, 3);
assert.equal(plugin.rateLimit[0].pathMatcher('/two-factor/verify-totp'), true);
const auth = betterAuth({
  ...reference.config,
  plugins: [...reference.config.plugins, plugin],
  rateLimit: { enabled: false },
  logger: { disabled: true }
});
// The plugin rate-rule metadata is asserted above; disabling enforcement here isolates attempts/lockout.
// This explicitly adds a plugin absent from the ForkLaunch preset; no preset claim.
let jar = new Map();
let checks = 0;
async function request(route, body, origin = 'http://localhost:3000') {
  const response = await auth.handler(
    new Request('http://localhost:3000/api/auth' + route, {
      method: body ? 'POST' : 'GET',
      headers: {
        origin,
        'content-type': 'application/json',
        cookie: [...jar].map(([k, v]) => `${k}=${v}`).join('; ')
      },
      ...(body ? { body: JSON.stringify(body) } : {})
    })
  );
  for (const c of response.headers.getSetCookie()) {
    const p = c.split(';')[0],
      i = p.indexOf('=');
    if (c.includes('Max-Age=0')) jar.delete(p.slice(0, i));
    else jar.set(p.slice(0, i), p.slice(i + 1));
  }
  const text = await response.text();
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    data = text;
  }
  return {
    status: response.status,
    data,
    jwt: response.headers.get('set-auth-jwt'),
    cookies: response.headers.getSetCookie()
  };
}
function status(r, n) {
  assert.equal(r.status, n, JSON.stringify(r.data));
  checks++;
  return r.data;
}
const credentials = {
  email: 'totp@example.test',
  password: 'owned-fixture-password',
  name: 'TOTP',
  firstName: 'TOTP',
  lastName: 'Audit'
};
const disabledSend = await auth.handler(
  new Request('http://localhost:3000/api/auth/two-factor/send-otp', {
    method: 'POST',
    headers: { origin: 'http://localhost:3000' }
  })
);
assert.equal(disabledSend.status, 400);
assert.equal((await disabledSend.json()).code, 'OTP_NOT_CONFIGURED');
assert.equal(
  status(await request('/two-factor/send-otp', { trustDevice: 'bad' }), 400)
    .code,
  'VALIDATION_ERROR'
);
assert.equal(
  status(await request('/two-factor/verify-otp', {}), 400).code,
  'VALIDATION_ERROR'
);
assert.equal(
  status(await request('/two-factor/verify-otp', { code: '123456' }), 401).code,
  'INVALID_TWO_FACTOR_COOKIE'
);
status(await request('/sign-up/email', credentials), 200);
assert.equal(
  status(await request('/two-factor/verify-otp', { code: '123456' }), 400).code,
  'OTP_HAS_EXPIRED'
);
const originalSession = jar.get('better-auth.session_token');
const originalToken = reference.database.session[0].token;
status(await request('/two-factor/enable', { password: 'wrong' }), 400);
status(
  await request(
    '/two-factor/enable',
    { password: credentials.password },
    'https://wrong.example'
  ),
  403
);
const enrolled = status(
  await request('/two-factor/enable', { password: credentials.password }),
  200
);
assert.equal(enrolled.method, 'totp');
assert.equal(enrolled.backupCodes.length, 10);
assert.equal(reference.database.twoFactor.length, 1);
assert.equal(reference.database.twoFactor[0].verified, false);
assert.equal(reference.database.user[0].twoFactorEnabled, false);
assert.equal(jar.get('better-auth.session_token'), originalSession);
status(await request('/two-factor/get-totp-uri', { password: 'wrong' }), 400);
assert.equal(
  status(
    await request('/two-factor/get-totp-uri', {
      password: credentials.password
    }),
    200
  ).totpURI,
  enrolled.totpURI
);
const row = reference.database.twoFactor[0];
const ctx = await auth.$context;
const secret = await symmetricDecrypt({
  key: ctx.secretConfig,
  data: row.secret
});
assert.notEqual(row.secret, secret);
assert.ok(!row.backupCodes.includes(enrolled.backupCodes[0]));
const code = await createOTP(secret).totp();
status(await request('/two-factor/verify-totp', { code: 'bad' }), 401);
const enrollmentVerified = await request('/two-factor/verify-totp', { code });
assert.equal(enrollmentVerified.jwt, null);
const verified = status(enrollmentVerified, 200);
assert.equal(
  verified.token,
  originalToken,
  'enrollment body retains old session snapshot while cookie rotates'
);
assert.equal(verified.user.twoFactorEnabled, false);
assert.equal(reference.database.user[0].twoFactorEnabled, true);
assert.equal(reference.database.twoFactor[0].verified, true);
assert.notEqual(jar.get('better-auth.session_token'), originalSession);
assert.equal(reference.database.session.length, 1);
const enrolledSession = await request('/get-session');
status(enrolledSession, 200);
assert.equal(
  JSON.parse(Buffer.from(enrolledSession.jwt.split('.')[1], 'base64url'))
    .sessionId,
  reference.database.session[0].id
);
assert.notEqual(enrolledSession.data.session.token, originalToken);
status(await request('/two-factor/verify-totp', { code }), 200); // no TOTP timestep replay suppression for authenticated session
status(
  await request('/two-factor/enable', { password: credentials.password }),
  400
);
status(await request('/sign-out', {}), 200);
const challengeResponse = await request('/sign-in/email', credentials);
assert.equal(challengeResponse.jwt, null);
const challenge = status(challengeResponse, 200);
assert.deepEqual(challenge, {
  twoFactorRedirect: true,
  twoFactorMethods: ['totp']
});
assert.equal(reference.database.session.length, 0);
assert.ok(jar.has('better-auth.two_factor'));
assert.ok(!jar.has('better-auth.session_token'));
const pendingSession = await request('/get-session');
status(pendingSession, 200);
assert.equal(pendingSession.data, null);
assert.equal(pendingSession.jwt, null);
const challengeJar = new Map(jar);
status(
  await request('/two-factor/verify-totp', { code, trustDevice: true }),
  200
);
assert.equal(reference.database.session.length, 1);
assert.ok(jar.has('better-auth.trust_device'));
assert.ok(!jar.has('better-auth.two_factor'));
const signedInJar = new Map(jar);
jar = challengeJar;
status(await request('/two-factor/verify-totp', { code }), 401);
jar = signedInJar;
const trustBefore = jar.get('better-auth.trust_device');
status(await request('/sign-out', {}), 200);
const trusted = status(await request('/sign-in/email', credentials), 200);
assert.ok(trusted.token);
assert.notEqual(jar.get('better-auth.trust_device'), trustBefore);
jar.delete('better-auth.trust_device');
status(await request('/sign-out', {}), 200);
status(await request('/sign-in/email', credentials), 200);
status(
  await request('/two-factor/verify-backup-code', {
    code: enrolled.backupCodes[0]
  }),
  200
);
status(await request('/sign-out', {}), 200);
status(await request('/sign-in/email', credentials), 200);
status(
  await request('/two-factor/verify-backup-code', {
    code: enrolled.backupCodes[0]
  }),
  401
);
status(await request('/two-factor/verify-totp', { code }), 200);

// A distinct challenge can reuse the current TOTP, but one challenge cannot issue two sessions.
status(await request('/sign-out', {}), 200);
status(await request('/sign-in/email', credentials), 200);
const raced = await Promise.all([
  request('/two-factor/verify-totp', { code }),
  request('/two-factor/verify-totp', { code })
]);
assert.deepEqual(raced.map((r) => r.status).sort(), [200, 401]);
checks += 2;
assert.equal(reference.database.session.length, 1);
// Default account lockout aggregates failures across new challenges.
for (let round = 0; round < 2; round++) {
  status(await request('/sign-out', {}), 200);
  status(await request('/sign-in/email', credentials), 200);
  for (let i = 0; i < 5; i++)
    status(await request('/two-factor/verify-totp', { code: 'invalid' }), 401);
  status(
    await request('/two-factor/verify-totp', { code }),
    round === 0 ? 400 : 429
  );
}
assert.equal(reference.database.twoFactor[0].failedVerificationCount, 10);
status(await request('/sign-in/email', credentials), 200);
status(await request('/two-factor/verify-totp', { code }), 429);
clock += 900001;
status(await request('/sign-in/email', credentials), 200);
status(
  await request('/two-factor/verify-totp', {
    code: await createOTP(secret).totp()
  }),
  200
);
assert.equal(reference.database.twoFactor[0].failedVerificationCount, 0);
assert.equal(reference.database.twoFactor[0].lockedUntil, null);
status(
  await request('/two-factor/generate-backup-codes', { password: 'wrong' }),
  400
);
const regenerated = status(
  await request('/two-factor/generate-backup-codes', {
    password: credentials.password
  }),
  200
);
assert.equal(regenerated.status, true);
assert.equal(regenerated.backupCodes.length, 10);
status(
  await request('/two-factor/verify-backup-code', {
    code: enrolled.backupCodes[1]
  }),
  401
);
status(
  await request('/two-factor/verify-backup-code', {
    code: regenerated.backupCodes[0]
  }),
  200
);
status(await request('/two-factor/disable', { password: 'wrong' }), 400);
status(
  await request('/two-factor/disable', { password: credentials.password }),
  200
);
assert.equal(reference.database.twoFactor.length, 0);
assert.equal(reference.database.user[0].twoFactorEnabled, false);
status(await request('/sign-out', {}), 200);
assert.ok(status(await request('/sign-in/email', credentials), 200).token);
// Actual client plugin; only the in-process HTTP transport and cookie jar are owned.
const { createAuthClient } = await load('better-auth/dist/client/index.mjs');
const { twoFactorClient } = await load(
  'better-auth/dist/plugins/two-factor/client.mjs'
);
let redirected = 0,
  clientCalls = 0;
const client = createAuthClient({
  baseURL: 'http://localhost:3000/api/auth',
  plugins: [
    twoFactorClient({
      onTwoFactorRedirect: ({ twoFactorMethods }) => {
        assert.deepEqual(twoFactorMethods, ['totp']);
        redirected++;
      }
    })
  ],
  fetchOptions: {
    customFetchImpl: async (url, options) => {
      clientCalls++;
      const headers = new Headers(options.headers);
      headers.set('origin', 'http://localhost:3000');
      headers.set('cookie', [...jar].map(([k, v]) => `${k}=${v}`).join('; '));
      const response = await auth.handler(
        new Request(url, { ...options, headers })
      );
      for (const c of response.headers.getSetCookie()) {
        const p = c.split(';')[0],
          i = p.indexOf('=');
        if (c.includes('Max-Age=0')) jar.delete(p.slice(0, i));
        else jar.set(p.slice(0, i), p.slice(i + 1));
      }
      return response;
    }
  }
});
const oldClientToken = reference.database.session[0].token;
assert.equal(
  (await client.twoFactor.enable({ password: credentials.password })).error,
  null
);
const clientSecret = await symmetricDecrypt({
  key: ctx.secretConfig,
  data: reference.database.twoFactor[0].secret
});
const clientCode = await createOTP(clientSecret).totp();
const enabledResult = await client.twoFactor.verifyTotp({ code: clientCode });
assert.equal(enabledResult.error, null);
assert.equal(enabledResult.data.token, oldClientToken);
const currentResult = await client.getSession();
assert.equal(currentResult.error, null);
assert.notEqual(currentResult.data.session.token, oldClientToken);
assert.equal(currentResult.data.user.twoFactorEnabled, true);
assert.equal((await client.signOut()).error, null);
assert.equal((await client.signIn.email(credentials)).error, null);
assert.equal(redirected, 1);
assert.equal(
  (await client.twoFactor.verifyTotp({ code: clientCode })).error,
  null
);
assert.equal(
  (await client.twoFactor.disable({ password: credentials.password })).error,
  null
);
assert.equal(clientCalls, 7);
console.log(
  `PASS actual twoFactorClient: ${clientCalls} HTTP calls, enrollment stale-body/new-cookie session, redirect callback, challenge completion and disable`
);
console.log(
  `PASS two-factor lifecycle ${checks} HTTP statuses; enrollment/verification/session rotation/challenge/recovery/trust/replay/origin/encryption`
);

// Separate handler enables the actual plugin rate rule; unrelated lifecycle calls cannot consume it.
const limited = betterAuth({
  ...reference.config,
  plugins: [...reference.config.plugins, twoFactor()],
  rateLimit: { enabled: true },
  logger: { disabled: true }
});
for (let index = 0; index < 4; index++) {
  const response = await limited.handler(
    new Request('http://localhost:3000/api/auth/two-factor/get-totp-uri', {
      method: 'POST',
      headers: {
        origin: 'http://localhost:3000',
        'content-type': 'application/json',
        'x-forwarded-for': '127.0.0.91',
        cookie: [...jar].map(([k, v]) => `${k}=${v}`).join('; ')
      },
      body: JSON.stringify({ password: credentials.password })
    })
  );
  assert.equal(response.status, index < 3 ? 400 : 429);
  if (index === 3) assert.equal(response.headers.get('x-retry-after'), '10');
}
console.log(
  'PASS actual two-factor plugin rate enforcement: 3 requests/10 seconds, fourth429 with X-Retry-After=10'
);
const observed = {};
const ttl = (session) =>
  (new RealDate(session.expiresAt) - new RealDate(session.createdAt)) / 1000;
const activeTtl = async () =>
  ttl(status(await request('/get-session'), 200).session);
status(await request('/sign-out', {}), 200);
status(
  await request('/sign-in/email', { ...credentials, rememberMe: false }),
  200
);
observed.ordinaryRememberFalseTtl = await activeTtl();
const beforeEnrollment = status(await request('/get-session'), 200).session;
clock += 300000;
status(
  await request('/two-factor/enable', { password: credentials.password }),
  200
);
const edgeSecret = await symmetricDecrypt({
  key: ctx.secretConfig,
  data: reference.database.twoFactor[0].secret
});
const edgeCode = await createOTP(edgeSecret).totp();
const enrolledFalse = await request('/two-factor/verify-totp', {
  code: edgeCode
});
status(enrolledFalse, 200);
observed.enrollmentRememberFalseTtl = await activeTtl();
const afterEnrollment = status(await request('/get-session'), 200).session;
observed.enrollmentPreservesExpiry =
  beforeEnrollment.expiresAt === afterEnrollment.expiresAt;
observed.enrollmentPreservesCreated =
  beforeEnrollment.createdAt === afterEnrollment.createdAt;
observed.enrollmentPersistentCookie = enrolledFalse.cookies
  .find((c) => c.startsWith('better-auth.session_token='))
  .includes('Max-Age=');
status(await request('/sign-out', {}), 200);
status(
  await request('/sign-in/email', { ...credentials, rememberMe: false }),
  200
);
observed.falseChallengeMarker = jar.has('better-auth.dont_remember');
status(
  await request('/sign-in/email', { ...credentials, rememberMe: true }),
  200
);
observed.trueChallengeRetainsMarker = jar.has('better-auth.dont_remember');
const inheritedCompletion = await request('/two-factor/verify-totp', {
  code: edgeCode
});
status(inheritedCompletion, 200);
observed.inheritedTrueChallengeTtl = await activeTtl();
observed.inheritedTruePersistentCookie = inheritedCompletion.cookies
  .find((c) => c.startsWith('better-auth.session_token='))
  .includes('Max-Age=');
status(await request('/sign-out', {}), 200);
status(
  await request('/sign-in/email', { ...credentials, rememberMe: false }),
  200
);
const trustedCompletion = await request('/two-factor/verify-totp', {
  code: edgeCode,
  trustDevice: true
});
status(trustedCompletion, 200);
observed.trustFalseTtl = await activeTtl();
observed.trustFalsePersistentCookie = trustedCompletion.cookies
  .find((c) => c.startsWith('better-auth.session_token='))
  .includes('Max-Age=');
observed.trustClearsMarker = !jar.has('better-auth.dont_remember');
status(await request('/sign-out', {}), 200);
status(
  await request('/sign-in/email', { ...credentials, rememberMe: false }),
  200
);
observed.trustedSigninFalseTtl = await activeTtl();
const beforeDisable = status(await request('/get-session'), 200).session;
clock += 300000;
status(
  await request('/two-factor/disable', { password: credentials.password }),
  200
);
const afterDisable = status(await request('/get-session'), 200).session;
observed.disablePreservesExpiry =
  beforeDisable.expiresAt === afterDisable.expiresAt;
observed.disablePreservesCreated =
  beforeDisable.createdAt === afterDisable.createdAt;
assert.deepEqual(observed, {
  ordinaryRememberFalseTtl: 86400,
  enrollmentRememberFalseTtl: 86400,
  enrollmentPreservesExpiry: false,
  enrollmentPreservesCreated: false,
  enrollmentPersistentCookie: false,
  falseChallengeMarker: true,
  trueChallengeRetainsMarker: true,
  inheritedTrueChallengeTtl: 86400,
  inheritedTruePersistentCookie: false,
  trustFalseTtl: 86400,
  trustFalsePersistentCookie: false,
  trustClearsMarker: true,
  trustedSigninFalseTtl: 86400,
  disablePreservesExpiry: false,
  disablePreservesCreated: false
});
console.log(
  'PASS elapsed session rotation, inherited dont_remember and transient trusted-device cookies'
);
const extraEnrollment = (
  await client.twoFactor.enable({
    password: credentials.password,
    issuer: 'Custom / Ü: Issuer',
    method: 'totp'
  })
).data;
assert.equal(
  new URL(extraEnrollment.totpURI).searchParams.get('issuer'),
  'Custom / Ü: Issuer'
);
const extraSecret = await symmetricDecrypt({
  key: ctx.secretConfig,
  data: reference.database.twoFactor[0].secret
});
const extraCode = await createOTP(extraSecret).totp();
assert.equal(
  (await client.twoFactor.verifyTotp({ code: extraCode })).error,
  null
);
assert.equal(
  new URL(
    (await client.twoFactor.getTotpUri({ password: credentials.password })).data
      .totpURI
  ).searchParams.get('issuer'),
  'Better Auth'
);
const tokenBefore = (await client.getSession()).data.session.token;
const countBefore = reference.database.session.length;
const authBackup = await client.twoFactor.verifyBackupCode({
  code: extraEnrollment.backupCodes[0],
  disableSession: true,
  trustDevice: true
});
assert.equal(authBackup.error, null);
assert.equal(authBackup.data.token, tokenBefore);
assert.equal(reference.database.session.length, countBefore);
await client.signOut();
await client.signIn.email(credentials);
assert.equal(reference.database.session.length, countBefore - 1);
const pendingBackup = await client.twoFactor.verifyBackupCode({
  code: extraEnrollment.backupCodes[1],
  disableSession: true,
  trustDevice: true
});
assert.equal(pendingBackup.error, null);
assert.ok(!Object.hasOwn(pendingBackup.data, 'token'));
assert.ok(!jar.has('better-auth.session_token'));
assert.ok(!jar.has('better-auth.trust_device'));
assert.ok(jar.has('better-auth.two_factor'));
const following = await client.twoFactor.verifyTotp({ code: extraCode });
assert.equal(following.error.status, 401);
assert.equal(following.error.code, 'INVALID_TWO_FACTOR_COOKIE');
console.log(
  'PASS follow-on actual client: issuer override/get-URI reset; authenticated disableSession retains token; challenged disableSession consumes code/attempt capability without session or trust, later TOTP401'
);
globalThis.Date = RealDate;

await import('./fixtures/rust_better_auth_totp_options.mjs');

await import('./fixtures/rust_better_auth_lifecycle_options.mjs');

await import('./fixtures/rust_better_auth_lifetime_options.mjs');

await import('./fixtures/rust_better_auth_lockout_options.mjs');

await import('./fixtures/rust_better_auth_backup_options.mjs');

await import('./fixtures/rust_better_auth_otp_storage.mjs');

await import('./fixtures/rust_better_auth_otp_encryption.mjs');

await import('./fixtures/rust_better_auth_otp_async.mjs');

await import('./fixtures/rust_better_auth_backup_storage.mjs');

await import('./fixtures/rust_better_auth_backup_async.mjs');

await import('./fixtures/rust_better_auth_backup_view.mjs');

await import('./fixtures/rust_better_auth_otp_background.mjs');
