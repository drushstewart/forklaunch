import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../..'
);
const prefix =
  process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/';
const observations = [];
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
const RealDate = Date;
let clock = 1800000000123;
globalThis.Date = class extends RealDate {
  constructor(...a) {
    super(...(a.length ? a : [clock]));
  }
  static now() {
    return clock;
  }
  static [Symbol.hasInstance](v) {
    return v instanceof RealDate;
  }
};
for (const cfg of [
  { totpOptions: { digits: 8, period: 15 } },
  { totpOptions: { digits: 6, period: 1.5 } },
  { totpOptions: { digits: 8, period: 60 }, skipVerificationOnEnable: true },
  { totpOptions: { disable: true } },
  { issuer: 'Global issuer', totpOptions: { issuer: 'URI issuer' } },
  { allowPasswordless: true }
]) {
  const ref = await betterAuthReference(root);
  ref.database.twoFactor = [];
  const auth = betterAuth({
    ...ref.config,
    plugins: [...ref.config.plugins, twoFactor(cfg)],
    logger: { disabled: true },
    rateLimit: { enabled: false }
  });
  let jar = new Map();
  async function call(route, body, authn = true) {
    const r = await auth.handler(
      new Request('http://localhost:3000/api/auth' + route, {
        method: 'POST',
        headers: {
          origin: 'http://localhost:3000',
          'content-type': 'application/json',
          cookie: authn ? [...jar].map(([k, v]) => k + '=' + v).join('; ') : ''
        },
        body: JSON.stringify(body)
      })
    );
    if (authn)
      for (const c of r.headers.getSetCookie()) {
        const pair = c.split(';')[0],
          i = pair.indexOf('=');
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
    name: 'OTP',
    firstName: 'OTP',
    lastName: 'User',
    email: 'totp-options@example.test',
    password: 'owned-factor-password'
  };
  await call('/sign-up/email', credentials);
  if (cfg.totpOptions?.disable) {
    const disabled = {
      verifyAnon: await call(
        '/two-factor/verify-totp',
        { code: '123456' },
        false
      ),
      uriAnon: await call(
        '/two-factor/get-totp-uri',
        { password: credentials.password },
        false
      ),
      enableBad: await call('/two-factor/enable', { password: 'wrong' }),
      enable: await call('/two-factor/enable', {
        password: credentials.password
      }),
      uri: await call('/two-factor/get-totp-uri', {
        password: credentials.password
      })
    };
    assert.deepEqual(
      Object.values(disabled).map((r) => [r.status, r.body.code]),
      [
        [400, 'TOTP_NOT_CONFIGURED'],
        [401, 'UNAUTHORIZED'],
        [400, 'INVALID_PASSWORD'],
        [400, 'TOTP_NOT_CONFIGURED'],
        [400, 'TOTP_NOT_CONFIGURED']
      ]
    );
    observations.push({ cfg, disabled });
    continue;
  }
  if (cfg.allowPasswordless) {
    assert.equal(
      (await call('/two-factor/enable', {})).body.code,
      'INVALID_PASSWORD'
    );
    ref.database.account = [];
    assert.equal((await call('/two-factor/enable', {})).status, 200);
    observations.push({
      cfg,
      credentialStillRequiresPassword: true,
      withoutAccountAllowed: true
    });
    continue;
  }
  const enable = await call('/two-factor/enable', {
    password: credentials.password
  });
  assert.equal(enable.status, 200);
  const row = ref.database.twoFactor[0],
    ctx = await auth.$context;
  const secret = await symmetricDecrypt({
    key: ctx.secretConfig,
    data: row.secret
  });
  const digits = cfg.totpOptions?.digits || 6,
    period = cfg.totpOptions?.period || 30;
  const code = await createOTP(secret, { digits, period }).totp();
  assert.equal(code.length, digits);
  const uri = await call('/two-factor/get-totp-uri', {
    password: credentials.password
  });
  const before = {
    verified: row.verified,
    enabled: ref.database.user[0].twoFactorEnabled,
    sessionCount: ref.database.session.length
  };
  assert.equal((await call('/two-factor/verify-totp', { code })).status, 200);
  const initial = clock;
  clock += period * 1000;
  assert.equal((await call('/two-factor/verify-totp', { code })).status, 200);
  clock += period * 1000;
  assert.equal((await call('/two-factor/verify-totp', { code })).status, 401);
  clock = initial;
  assert.equal(before.verified, !!cfg.skipVerificationOnEnable);
  assert.equal(before.enabled, !!cfg.skipVerificationOnEnable);
  assert.equal(before.sessionCount, 1);
  assert.equal(enable.cookies, cfg.skipVerificationOnEnable ? 1 : 0);
  assert.equal(
    new URL(enable.body.totpURI).searchParams.get('issuer'),
    cfg.issuer || 'Better Auth'
  );
  assert.equal(
    new URL(uri.body.totpURI).searchParams.get('issuer'),
    cfg.totpOptions?.issuer || 'Better Auth'
  );
  observations.push({
    cfg,
    uriDigits: new URL(enable.body.totpURI).searchParams.get('digits'),
    uriPeriod: new URL(enable.body.totpURI).searchParams.get('period'),
    enableIssuer: new URL(enable.body.totpURI).searchParams.get('issuer'),
    getIssuer: new URL(uri.body.totpURI).searchParams.get('issuer'),
    before,
    enableCookies: enable.cookies,
    window: 'one step accepted / two rejected'
  });
}
globalThis.Date = RealDate;

const vectors = JSON.parse(
  await readFile(
    path.join(
      root,
      'blueprint-rs/crates/implementations/iam/forklaunch-iam-better-auth/src/two_factor/tests/totp_vectors.json'
    ),
    'utf8'
  )
);
assert.equal(vectors.length, 96);
for (const vector of vectors) {
  const { digits, periodMs, millis, counter, code } = vector;
  assert.equal(Math.floor(millis / ((periodMs / 1000) * 1000)), counter);
  assert.equal(
    await createOTP('12345678901234567890', {
      digits,
      period: periodMs / 1000
    }).hotp(counter),
    code
  );
}
assert.equal(Math.floor(200700 / (2.007 * 1000)), 99);
// Actual upstream wraps a negative HOTP counter to unsigned64; native explicitly
// keeps its historical fail-closed boundary instead of accepting that epoch edge.
assert.equal(
  await createOTP('12345678901234567890').hotp(-1),
  await createOTP('12345678901234567890').hotp(18446744073709551615n)
);
console.log(
  `PASS actual TOTP options: ${observations.length} lifecycle profiles, ${vectors.length} SHA1/counter vectors, explicit unsigned-wrap boundary`
);
