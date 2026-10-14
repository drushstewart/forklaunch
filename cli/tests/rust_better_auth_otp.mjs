import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
assert.ok(process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES);
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
const RealDate = Date;
let clock = 1800000000000;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [clock]));
  }
  // structuredClone returns native Dates; preserve their instanceof semantics.
  static [Symbol.hasInstance](value) {
    return value instanceof RealDate;
  }
  static now() {
    return clock;
  }
};
// Suppress only this fixture's deliberate synthetic synchronous callback stack.
const originalError = console.error;
console.error = () => {};
const results = [];
for (const mode of [
  'success',
  'syncThrow',
  'asyncReject',
  'pendingAwait',
  'pendingBackground'
]) {
  const ref = await betterAuthReference(root);
  ref.database.twoFactor = [];
  let delivered;
  let deliveryCalls = 0;
  let releaseDelivery;
  const deliveryPending = new Promise((resolve) => {
    releaseDelivery = resolve;
  });
  const sendOTP = ({ otp }) => {
    deliveryCalls++;
    delivered = otp;
    if (mode === 'syncThrow') throw Error('fixture secret failure');
    if (mode === 'asyncReject')
      return Promise.reject(Error('fixture secret failure'));
    return mode.startsWith('pending') ? deliveryPending : Promise.resolve();
  };
  const auth = betterAuth({
    ...ref.config,
    plugins: [
      ...ref.config.plugins,
      twoFactor(mode === 'absent' ? {} : { otpOptions: { sendOTP } })
    ],
    advanced: {
      ...ref.config.advanced,
      ...(mode === 'pendingBackground'
        ? {
            backgroundTasks: {
              handler: (promise) => {
                promise.catch(() => {});
              }
            }
          }
        : {})
    },
    rateLimit: { enabled: false },
    logger: { disabled: true }
  });
  const jar = new Map();
  async function call(route, body, authenticated = true) {
    const r = await auth.handler(
      new Request('http://localhost:3000/api/auth' + route, {
        method: 'POST',
        headers: {
          origin: 'http://localhost:3000',
          'content-type': 'application/json',
          cookie: authenticated
            ? [...jar].map(([k, v]) => `${k}=${v}`).join('; ')
            : ''
        },
        ...(body === undefined ? {} : { body: JSON.stringify(body) })
      })
    );
    for (const c of r.headers.getSetCookie()) {
      const [pair] = c.split(';');
      const i = pair.indexOf('=');
      if (c.includes('Max-Age=0')) jar.delete(pair.slice(0, i));
      else jar.set(pair.slice(0, i), pair.slice(i + 1));
    }
    const text = await r.text();
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
    return { status: r.status, data, cookies: r.headers.getSetCookie().length };
  }
  const out = { mode };
  out.unauthSend = await call('/two-factor/send-otp', undefined, false);
  out.unauthVerify = await call(
    '/two-factor/verify-otp',
    { code: '123456' },
    false
  );
  out.invalidSend = await call(
    '/two-factor/send-otp',
    { trustDevice: 'bad' },
    false
  );
  const credentials = {
    name: 'OTP',
    firstName: 'OTP',
    lastName: 'User',
    email: 'otp@example.test',
    password: 'owned-factor-password'
  };
  assert.equal((await call('/sign-up/email', credentials)).status, 200);
  out.authVerifyMissing = await call('/two-factor/verify-otp', {
    code: '123456'
  });
  if (mode.startsWith('pending')) {
    let responded = false;
    const sending = call('/two-factor/send-otp', {}).then((r) => {
      responded = true;
      return r;
    });
    await new Promise((resolve) => setTimeout(resolve, 30));
    out.respondedBeforeDelivery = responded;
    releaseDelivery();
    out.send = await sending;
    assert.equal(out.respondedBeforeDelivery, mode === 'pendingBackground');
  } else out.send = await call('/two-factor/send-otp', {});
  out.deliveryCalls = deliveryCalls;
  out.otpRows = ref.database.verification.filter((r) =>
    r.identifier.startsWith('2fa-otp-')
  ).length;
  if (delivered) {
    out.verify = await call('/two-factor/verify-otp', { code: delivered });
    out.verify.data = {
      hasToken: !!out.verify.data.token,
      twoFactorEnabled: out.verify.data.user?.twoFactorEnabled
    };
    out.replay = await call('/two-factor/verify-otp', { code: delivered });
  }
  out.enableOtp = await call('/two-factor/enable', {
    password: credentials.password,
    method: 'otp'
  });
  out.factorRows = ref.database.twoFactor.length;
  await call('/sign-out', {});
  out.signin = await call('/sign-in/email', credentials);
  out.signin.data = {
    twoFactorRedirect: out.signin.data.twoFactorRedirect,
    twoFactorMethods: out.signin.data.twoFactorMethods
  };
  if (mode === 'success') {
    out.pendingSendA = await call('/two-factor/send-otp', {});
    const first = delivered;
    // Distinct creation times establish upstream's newest-row ordering; equal
    // timestamps leave adapter tie ordering unspecified.
    clock += 1;
    out.pendingSendB = await call('/two-factor/send-otp', {});
    const second = delivered;
    out.pendingRows = ref.database.verification.filter((r) =>
      r.identifier.startsWith('2fa-otp-')
    ).length;
    out.firstResentCode = await call('/two-factor/verify-otp', { code: first });
    out.firstResentCode.data = {
      code: out.firstResentCode.data.code,
      hasToken: !!out.firstResentCode.data.token
    };
    out.secondResentCode = await call('/two-factor/verify-otp', {
      code: second
    });
    out.secondResentCode.data = {
      code: out.secondResentCode.data.code,
      hasToken: !!out.secondResentCode.data.token
    };
    await call('/sign-out', {});
    await call('/sign-in/email', credentials);
    await call('/two-factor/send-otp', {});
    out.badAttempts = [];
    for (let i = 0; i < 7; i++) {
      const r = await call('/two-factor/verify-otp', { code: 'not-code' });
      out.badAttempts.push([r.status, r.data.code]);
    }
  }
  if (mode === 'success') {
    await call('/two-factor/send-otp', {});
    const expiring = delivered;
    clock += 180000;
    out.expiryEquality = await call('/two-factor/verify-otp', {
      code: expiring
    });
    out.expiryEquality.data = {
      code: out.expiryEquality.data.code,
      hasToken: !!out.expiryEquality.data.token
    };
    await call('/sign-out', {});
    await call('/sign-in/email', credentials);
    await call('/two-factor/send-otp', {});
    const racing = delivered;
    out.concurrent = await Promise.all([
      call('/two-factor/verify-otp', { code: racing }),
      call('/two-factor/verify-otp', { code: racing })
    ]);
    out.concurrent = out.concurrent.map((r) => ({
      status: r.status,
      code: r.data.code,
      hasToken: !!r.data.token
    }));
  }
  assert.equal(out.unauthSend.status, 401);
  assert.equal(out.unauthVerify.status, 401);
  assert.equal(out.invalidSend.status, 400);
  assert.equal(out.authVerifyMissing.data.code, 'OTP_HAS_EXPIRED');
  assert.equal(out.send.status, mode === 'syncThrow' ? 500 : 200);
  assert.equal(out.otpRows, 1);
  assert.equal(out.verify.status, 200);
  assert.equal(out.verify.data.twoFactorEnabled, true);
  assert.equal(out.replay.data.code, 'OTP_HAS_EXPIRED');
  assert.equal(out.enableOtp.status, 200);
  assert.equal(out.factorRows, 0);
  assert.deepEqual(out.signin.data.twoFactorMethods, ['otp']);
  if (mode === 'success') {
    assert.equal(out.firstResentCode.status, 401);
    assert.equal(out.secondResentCode.status, 200);
    assert.deepEqual(
      out.badAttempts,
      Array(5)
        .fill([401, 'INVALID_CODE'])
        .concat([
          [400, 'TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE'],
          [400, 'OTP_HAS_EXPIRED']
        ])
    );
    assert.equal(out.expiryEquality.status, 200);
    assert.deepEqual(out.concurrent.map((r) => r.status).sort(), [200, 401]);
  }
  results.push(out);
}
globalThis.Date = RealDate;
console.error = originalError;
console.log(
  `PASS actual OTP: ${results.length} callback profiles; enrollment, replacement, replay, resend, five attempts, expiry equality, concurrent proof, default await/background dispatch`
);
