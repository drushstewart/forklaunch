// Actual Better Auth client plugin -> native production HTTP handlers. The owned
// JSON-line transport supplies HTTP/cookies; this is not a browser or TCP claim.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const prefix =
  process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/';
const load = (p) => import(pathToFileURL(prefix + p));
const { createAuthClient } = await load('better-auth/dist/client/index.mjs');
const { twoFactorClient } = await load(
  'better-auth/dist/plugins/two-factor/client.mjs'
);
const { createOTP } = await load('@better-auth/utils/dist/otp.mjs');
const { base32 } = await load('@better-auth/utils/dist/base32.mjs');
const { jwtVerify, createLocalJWKSet } = await load(
  'jose/dist/webapi/index.js'
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
const RealDate = Date,
  now = 1800000000000;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [now]));
  }
  static now() {
    return now;
  }
};
const jar = new Map();
let calls = 0,
  redirects = 0;
const history = [];
const transport = async (url, options = {}) => {
  calls++;
  const headers = new Headers(options.headers);
  headers.set('origin', 'http://localhost:3000');
  headers.set('content-type', 'application/json');
  headers.set('cookie', [...jar].map(([k, v]) => `${k}=${v}`).join('; '));
  const result = await native({
    op: 'request',
    now,
    path: new URL(url).pathname.replace(/^\/api\/auth/, ''),
    method: options.method ?? 'GET',
    headers: Object.fromEntries(headers),
    body: typeof options.body === 'string' ? options.body : ''
  });
  const responseHeaders = new Headers();
  for (const [k, values] of Object.entries(result.headers))
    for (const v of values) responseHeaders.append(k, v);
  for (const cookie of result.headers['set-cookie'] ?? []) {
    const [name, ...tail] = cookie.split(';')[0].split('=');
    if (cookie.includes('Max-Age=0')) jar.delete(name);
    else jar.set(name, tail.join('='));
  }
  history.push({
    path: new URL(url).pathname,
    status: result.status,
    jwt: responseHeaders.get('set-auth-jwt')
  });
  return new Response(JSON.stringify(result.body), {
    status: result.status,
    headers: responseHeaders
  });
};
const client = createAuthClient({
  baseURL: 'http://localhost:3000/api/auth',
  plugins: [
    twoFactorClient({
      onTwoFactorRedirect: ({ twoFactorMethods }) => {
        assert.deepEqual(twoFactorMethods, ['totp']);
        redirects++;
      }
    })
  ],
  fetchOptions: { customFetchImpl: transport }
});
const ok = (result) => {
  assert.equal(result.error, null, JSON.stringify(result.error));
  return result.data;
};
try {
  const credentials = {
    name: 'Native Factor',
    firstName: 'Native',
    lastName: 'Factor',
    email: 'native-factor@example.test',
    password: 'owned-factor-password'
  };
  const signup = ok(await client.signUp.email(credentials));
  assert.equal(signup.user.twoFactorEnabled, false);
  const enrolled = ok(
    await client.twoFactor.enable({ password: credentials.password })
  );
  assert.equal(enrolled.backupCodes.length, 10);
  const secret = base32.decode(
    new URL(enrolled.totpURI).searchParams.get('secret')
  );
  const otpKey = await crypto.subtle.importKey(
    'raw',
    secret,
    { name: 'HMAC', hash: 'SHA-1' },
    false,
    ['sign']
  );
  const code = await createOTP(otpKey).totp();
  const verified = ok(await client.twoFactor.verifyTotp({ code }));
  assert.equal(verified.token, signup.token);
  assert.equal(verified.user.twoFactorEnabled, false);
  const active = ok(await client.getSession());
  assert.notEqual(active.session.token, signup.token);
  assert.equal(active.user.twoFactorEnabled, true);
  const jwt = history.at(-1).jwt;
  assert.ok(jwt);
  const jwks = await (
    await transport('http://localhost:3000/api/auth/jwks')
  ).json();
  const { payload } = await jwtVerify(jwt, createLocalJWKSet(jwks), {
    currentDate: new RealDate(now),
    issuer: 'http://localhost:3000',
    audience: 'http://localhost:3000'
  });
  assert.equal(payload.sessionId, active.session.id);
  ok(await client.signOut());
  const challenge = ok(await client.signIn.email(credentials));
  assert.equal(challenge.twoFactorRedirect, true);
  assert.equal(redirects, 1);
  assert.equal(history.at(-1).jwt, null);
  assert.ok(!jar.has('better-auth.session_token'));
  assert.equal(ok(await client.getSession()), null);
  assert.equal(history.at(-1).jwt, null);
  ok(await client.twoFactor.verifyTotp({ code, trustDevice: true }));
  assert.ok(jar.has('better-auth.trust_device'));
  const oldTrust = jar.get('better-auth.trust_device');
  ok(await client.signOut());
  assert.ok(ok(await client.signIn.email(credentials)).token);
  assert.notEqual(jar.get('better-auth.trust_device'), oldTrust);
  assert.equal(redirects, 1);
  jar.delete('better-auth.trust_device');
  ok(await client.signOut());
  ok(await client.signIn.email(credentials));
  ok(
    await client.twoFactor.verifyBackupCode({ code: enrolled.backupCodes[0] })
  );
  ok(await client.signOut());
  ok(await client.signIn.email(credentials));
  assert.equal(
    (await client.twoFactor.verifyBackupCode({ code: enrolled.backupCodes[0] }))
      .error.status,
    401
  );
  ok(await client.twoFactor.verifyTotp({ code }));
  ok(await client.twoFactor.disable({ password: credentials.password }));
  assert.equal(ok(await client.getSession()).user.twoFactorEnabled, false);
  console.log(
    `PASS actual twoFactorClient/native HTTP: ${calls} calls, authoritative JWT, enrollment rotation, challenge, trust rotation, backup single-use and disable`
  );
} finally {
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
  lines.close();
}
