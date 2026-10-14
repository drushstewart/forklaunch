// Actual client/plugin lifecycle with an explicit owned cookie jar and transport.
// This proves HTTP client/server interoperability, not browser cookie enforcement.
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';

export async function exerciseTwoFactorClient({
  baseURL,
  origin,
  fetchImpl = fetch,
  clock = Date.now,
  label = 'actual twoFactorClient'
}) {
  assert.ok(process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES);
  const prefix =
    process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/';
  const load = (name) => import(pathToFileURL(prefix + name));
  const { createAuthClient } = await load('better-auth/dist/client/index.mjs');
  const { twoFactorClient } = await load(
    'better-auth/dist/plugins/two-factor/client.mjs'
  );
  const { createOTP } = await load('@better-auth/utils/dist/otp.mjs');
  const { base32 } = await load('@better-auth/utils/dist/base32.mjs');
  const { jwtVerify, createLocalJWKSet } = await load(
    'jose/dist/webapi/index.js'
  );
  const jar = new Map();
  const history = [];
  let calls = 0,
    redirects = 0;
  const transport = async (url, options = {}) => {
    calls++;
    const headers = new Headers(options.headers);
    headers.set('origin', origin);
    headers.set('content-type', 'application/json');
    headers.set(
      'cookie',
      [...jar].map(([key, value]) => `${key}=${value}`).join('; ')
    );
    const response = await fetchImpl(url, { ...options, headers });
    for (const cookie of response.headers.getSetCookie()) {
      const [name, ...tail] = cookie.split(';')[0].split('=');
      if (cookie.includes('Max-Age=0')) jar.delete(name);
      else jar.set(name, tail.join('='));
    }
    history.push({
      status: response.status,
      jwt: response.headers.get('set-auth-jwt'),
      cookies: response.headers.getSetCookie()
    });
    return response;
  };
  const client = createAuthClient({
    baseURL,
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
  const credentials = {
    name: 'Native Factor',
    firstName: 'Native',
    lastName: 'Factor',
    email: `native-factor-${crypto.randomUUID()}@example.test`,
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
  // The actual utility exposes HOTP; use the trusted clock's standard TOTP counter
  // instead of mutating process-global Date or assuming the workflow finishes in 30s.
  const code = () => createOTP(otpKey).hotp(Math.floor(clock() / 30000));
  const verified = ok(
    await client.twoFactor.verifyTotp({ code: await code() })
  );
  assert.equal(verified.token, signup.token);
  assert.equal(verified.user.twoFactorEnabled, false);
  const active = ok(await client.getSession());
  assert.notEqual(active.session.token, signup.token);
  assert.equal(active.user.twoFactorEnabled, true);
  const jwt = history.at(-1).jwt;
  assert.ok(jwt);
  const jwks = await (
    await transport(`${baseURL.replace(/\/$/, '')}/jwks`)
  ).json();
  const { payload } = await jwtVerify(jwt, createLocalJWKSet(jwks), {
    currentDate: new Date(clock()),
    issuer: origin,
    audience: origin
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
  ok(
    await client.twoFactor.verifyTotp({ code: await code(), trustDevice: true })
  );
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
  ok(await client.twoFactor.verifyTotp({ code: await code() }));
  const beforeSkip = ok(await client.getSession());
  const authenticatedSkip = ok(
    await client.twoFactor.verifyBackupCode({
      code: enrolled.backupCodes[1],
      disableSession: true,
      trustDevice: true
    })
  );
  assert.equal(authenticatedSkip.token, beforeSkip.session.token);
  assert.deepEqual(history.at(-1).cookies, []);
  ok(await client.signOut());
  ok(await client.signIn.email(credentials));
  const pendingSkip = ok(
    await client.twoFactor.verifyBackupCode({
      code: enrolled.backupCodes[2],
      disableSession: true,
      trustDevice: true
    })
  );
  assert.ok(!Object.hasOwn(pendingSkip, 'token'));
  assert.deepEqual(history.at(-1).cookies, []);
  assert.ok(!jar.has('better-auth.session_token'));
  assert.ok(!jar.has('better-auth.trust_device'));
  assert.ok(jar.has('better-auth.two_factor'));
  const consumed = await client.twoFactor.verifyTotp({ code: await code() });
  assert.equal(consumed.error.status, 401);
  assert.equal(consumed.error.code, 'INVALID_TWO_FACTOR_COOKIE');
  ok(await client.signIn.email({ ...credentials, rememberMe: false }));
  assert.ok(jar.has('better-auth.dont_remember'));
  ok(await client.signIn.email({ ...credentials, rememberMe: true }));
  assert.ok(jar.has('better-auth.dont_remember'));
  ok(
    await client.twoFactor.verifyTotp({ code: await code(), trustDevice: true })
  );
  const transientSessionCookie = history
    .at(-1)
    .cookies.find((cookie) => cookie.startsWith('better-auth.session_token='));
  assert.ok(transientSessionCookie);
  assert.ok(!transientSessionCookie.includes('Max-Age='));
  assert.ok(!jar.has('better-auth.dont_remember'));
  ok(await client.twoFactor.disable({ password: credentials.password }));
  assert.equal(ok(await client.getSession()).user.twoFactorEnabled, false);
  console.log(
    `PASS ${label}: ${calls} HTTP calls, authoritative JWT, enrollment rotation, challenge, trust rotation, backup single-use and disable`
  );
  return calls;
}
