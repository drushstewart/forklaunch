// Actual client/plugin -> native production handlers through the owned JSON-line bridge.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const child = spawn(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    'blueprint-rs/Cargo.toml',
    '-p',
    'forklaunch-iam-better-auth',
    '--features',
    'http',
    '--example',
    'http_parity'
  ],
  {
    cwd: root,
    env: {
      ...process.env,
      CARGO_INCREMENTAL: '0',
      FORKLAUNCH_AUTH_TWO_FACTOR: 'true',
      FORKLAUNCH_AUTH_OTP: 'true',
      NODE_ENV: 'test'
    },
    stdio: ['pipe', 'pipe', 'inherit']
  }
);
const pending = [];
const lines = createInterface({ input: child.stdout });
lines.on('line', (line) => pending.shift().resolve(JSON.parse(line)));
const completion = new Promise((resolve, reject) => {
  child.once('error', reject);
  child.once('exit', (code) => {
    if (code === 0) resolve();
    else reject(new Error(`Native TOTP bridge exited ${code}`));
    for (const call of pending.splice(0))
      call.reject(new Error('Native bridge closed'));
  });
});
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(JSON.stringify(input) + '\n');
  });

let now = 1800000000000;
const prefix =
  process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/';
const { createAuthClient } = await import(
  pathToFileURL(prefix + 'better-auth/dist/client/vanilla.mjs')
);
const { twoFactorClient } = await import(
  pathToFileURL(prefix + 'better-auth/dist/plugins/two-factor/client.mjs')
);
const jar = new Map();
let calls = 0;
let headers;
const transport = async (url, options = {}) => {
  const h = new Headers(options.headers);
  h.set('origin', 'http://localhost:3000');
  h.set('cookie', [...jar].map(([k, v]) => `${k}=${v}`).join('; '));
  const r = await native({
    op: 'request',
    now,
    path: new URL(url).pathname.replace(/^\/api\/auth/, ''),
    method: options.method ?? 'GET',
    headers: Object.fromEntries(h),
    body: options.body ?? ''
  });
  calls++;
  if (r.status === 500)
    assert.equal(r.body, '', 'callback preparation failure stays empty');
  headers = new Headers();
  for (const [name, values] of Object.entries(r.headers))
    for (const value of values) headers.append(name, value);
  for (const c of headers.getSetCookie()) {
    const pair = c.split(';')[0],
      i = pair.indexOf('=');
    if (c.includes('Max-Age=0')) jar.delete(pair.slice(0, i));
    else jar.set(pair.slice(0, i), pair.slice(i + 1));
  }
  return new Response(JSON.stringify(r.body), { status: r.status, headers });
};
const client = createAuthClient({
  baseURL: 'http://localhost:3000/api/auth',
  plugins: [
    twoFactorClient({
      onTwoFactorRedirect: ({ twoFactorMethods }) =>
        assert.deepEqual(twoFactorMethods, ['otp'])
    })
  ],
  fetchOptions: { customFetchImpl: transport }
});
const ok = (r) => {
  assert.equal(r.error, null, JSON.stringify(r.error));
  return r.data;
};
const code = async () => (await native({ op: 'otpDelivery', now })).code;
const credentials = {
  name: 'OTP',
  firstName: 'OTP',
  lastName: 'User',
  email: 'native-otp@example.test',
  password: 'owned-factor-password'
};
try {
  const signup = ok(await client.signUp.email(credentials));
  assert.deepEqual(
    ok(
      await client.twoFactor.enable({
        password: credentials.password,
        method: 'otp'
      })
    ),
    { method: 'otp' }
  );
  const enabled = ok(await client.getSession());
  assert.equal(enabled.user.twoFactorEnabled, true);
  assert.notEqual(enabled.session.token, signup.token);
  ok(await client.signOut());
  assert.equal(
    ok(await client.signIn.email(credentials)).twoFactorRedirect,
    true
  );
  assert.equal(headers.get('set-auth-jwt'), null);
  assert.ok(!jar.has('better-auth.session_token'));
  ok(await client.twoFactor.sendOtp());
  const first = await code();
  now++;
  ok(await client.twoFactor.sendOtp());
  const second = await code();
  if (first !== second)
    assert.equal(
      (await client.twoFactor.verifyOtp({ code: first })).error.code,
      'INVALID_CODE'
    );
  const completed = ok(
    await client.twoFactor.verifyOtp({ code: second, trustDevice: true })
  );
  assert.ok(completed.token);
  assert.ok(jar.has('better-auth.trust_device'));
  assert.equal(
    (await client.twoFactor.verifyOtp({ code: second })).error.code,
    'OTP_HAS_EXPIRED'
  );
  const current = ok(await client.getSession());
  assert.equal(current.session.token, completed.token);
  assert.ok(headers.get('set-auth-jwt'));
  for (const mode of [1, 2]) {
    await native({ op: 'otpDelivery', now, mode });
    const sent = await client.twoFactor.sendOtp();
    if (mode === 1) assert.equal(sent.error.status, 500);
    else ok(sent);
    assert.equal(
      ok(await client.twoFactor.verifyOtp({ code: await code() })).token,
      current.session.token
    );
  }
  await native({ op: 'otpDelivery', now, mode: 0 });
  ok(await client.twoFactor.sendOtp());
  now += 180000;
  ok(await client.twoFactor.verifyOtp({ code: await code() }));
  ok(await client.twoFactor.sendOtp());
  now += 180001;
  assert.equal(
    (await client.twoFactor.verifyOtp({ code: await code() })).error.code,
    'OTP_HAS_EXPIRED'
  );
  ok(await client.twoFactor.disable({ password: credentials.password }));
  assert.equal(ok(await client.getSession()).user.twoFactorEnabled, false);
  console.log(
    `PASS actual twoFactorClient/native OTP: ${calls} HTTP calls; OTP-only enrollment, challenge, resend, trust, failure modes, expiry and disable`
  );
} finally {
  child.stdin.end();
  await completion;
  lines.close();
}
