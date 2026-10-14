import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../..'
);
const { betterAuthReference } = await import(
  pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs')
);
const p =
  process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES +
  '/node_modules/better-auth/dist/';
const { betterAuth } = await import(pathToFileURL(p + 'index.mjs'));
const { twoFactor } = await import(
  pathToFileURL(p + 'plugins/two-factor/index.mjs')
);
const RealDate = Date;
let clock = 1800000000000;
globalThis.Date = class extends RealDate {
  constructor(...a) {
    super(...(a.length ? a : [clock]));
  }
  static now() {
    return clock;
  }
  static [Symbol.hasInstance](x) {
    return x instanceof RealDate;
  }
};
const observations = [];
for (const cfg of [
  { digits: 1, period: 1 / 60000, allowedAttempts: 1 },
  { digits: 32, period: 1, allowedAttempts: 100 },
  { digits: 4, period: 0.025, allowedAttempts: 1 },
  { digits: 8, period: 0.05, allowedAttempts: 3 },
  { digits: 6, period: 0, allowedAttempts: 0 }
]) {
  const ref = await betterAuthReference(root);
  ref.database.twoFactor = [];
  let code;
  const options = {
    ...cfg,
    sendOTP: ({ otp }) => {
      code = otp;
    }
  };
  const auth = betterAuth({
    ...ref.config,
    plugins: [...ref.config.plugins, twoFactor({ otpOptions: options })],
    rateLimit: { enabled: false },
    logger: { disabled: true }
  });
  let cookie = '';
  async function call(route, body) {
    const r = await auth.handler(
      new Request('http://localhost:3000/api/auth' + route, {
        method: 'POST',
        headers: {
          origin: 'http://localhost:3000',
          'content-type': 'application/json',
          cookie
        },
        body: JSON.stringify(body)
      })
    );
    for (const c of r.headers.getSetCookie()) {
      const pair = c.split(';')[0];
      if (pair.startsWith('better-auth.session_token=')) cookie = pair;
    }
    return { status: r.status, body: await r.json() };
  }
  await call('/sign-up/email', {
    name: 'OTP',
    firstName: 'OTP',
    lastName: 'User',
    email: 'opts@example.test',
    password: 'owned-factor-password'
  });
  const send = () => call('/two-factor/send-otp', {});
  const verify = (x) => call('/two-factor/verify-otp', { code: x });
  await send();
  assert.equal(code.length, cfg.digits);
  assert.match(code, /^[0-9]+$/);
  const row = ref.database.verification.find((r) =>
    r.identifier.startsWith('2fa-otp-')
  );
  const ms = (cfg.period || 3) * 60000;
  assert.equal(+row.expiresAt - clock, ms);
  const n = cfg.allowedAttempts || 5;
  for (let i = 0; i < n; i++)
    assert.equal((await verify('bad')).body.code, 'INVALID_CODE');
  assert.equal(
    (await verify(code)).body.code,
    'TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE'
  );
  assert.equal((await verify(code)).body.code, 'OTP_HAS_EXPIRED');
  clock += 10;
  await send();
  clock += ms;
  assert.equal((await verify(code)).status, 200);
  clock += 10;
  await send();
  clock += ms + 1;
  assert.equal((await verify(code)).body.code, 'OTP_HAS_EXPIRED');
  // allowedAttempts reads the original options object at verify time.
  clock += 10;
  await send();
  options.allowedAttempts = 1;
  assert.equal((await verify('bad')).body.code, 'INVALID_CODE');
  assert.equal(
    (await verify(code)).body.code,
    'TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE'
  );
  observations.push({
    ...cfg,
    expiryMs: ms,
    attempts: n,
    expiryEquality: 'accepted',
    policyMutation: 'observed'
  });
}
globalThis.Date = RealDate;
console.log(
  `PASS actual OTP options: ${observations.length} profiles, expiry boundaries, attempts and current-policy changes`
);

const ref = await betterAuthReference(root);
ref.database.twoFactor = [];
const auth = betterAuth({
  ...ref.config,
  plugins: [...ref.config.plugins, twoFactor()],
  rateLimit: { enabled: false },
  logger: { disabled: true }
});
let cookie = '';
async function call(route, body, withCookie = true) {
  const r = await auth.handler(
    new Request('http://localhost:3000/api/auth' + route, {
      method: 'POST',
      headers: {
        origin: 'http://localhost:3000',
        'content-type': 'application/json',
        cookie: withCookie ? cookie : ''
      },
      body: JSON.stringify(body)
    })
  );
  if (route === '/sign-up/email')
    cookie = r.headers
      .getSetCookie()
      .map((x) => x.split(';')[0])
      .join('; ');
  return { status: r.status, body: await r.text() };
}
assert.deepEqual(
  await call(
    '/two-factor/enable',
    { method: 'otp', password: 'owned-factor-password' },
    false
  ),
  {
    status: 401,
    body: JSON.stringify({ message: 'Unauthorized', code: 'UNAUTHORIZED' })
  }
);
await call('/sign-up/email', {
  name: 'OTP',
  firstName: 'OTP',
  lastName: 'User',
  email: 'order@example.test',
  password: 'owned-factor-password'
});
assert.deepEqual(
  await call('/two-factor/enable', { method: 'otp', password: 'wrong' }),
  {
    status: 400,
    body: JSON.stringify({
      message: 'Invalid password',
      code: 'INVALID_PASSWORD'
    })
  }
);
assert.deepEqual(
  await call('/two-factor/enable', {
    method: 'otp',
    password: 'owned-factor-password'
  }),
  {
    status: 400,
    body: JSON.stringify({
      message: 'OTP is not available',
      code: 'OTP_NOT_CONFIGURED'
    })
  }
);
console.log(
  'PASS actual no-hook OTP enrollment authority/password/configuration ordering'
);
