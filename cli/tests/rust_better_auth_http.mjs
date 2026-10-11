// Executes native production raw HTTP routes beside actual configured handlers.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const reference = await betterAuthReference(root);
const { jwtVerify, createLocalJWKSet } = await import(
  pathToFileURL(
    path.join(
      process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES,
      'node_modules/jose/dist/webapi/index.js',
    ),
  )
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
    'http_parity',
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    stdio: ['pipe', 'pipe', 'inherit'],
  },
);
const pending = [];
const lines = createInterface({ input: child.stdout });
lines.on('line', (line) => pending.shift().resolve(JSON.parse(line)));
const completion = new Promise((resolve, reject) => {
  child.once('error', reject);
  child.once('exit', (code) => {
    if (code === 0) resolve();
    else reject(new Error(`Auth HTTP bridge exited ${code}`));
    for (const next of pending.splice(0))
      next.reject(new Error('Native bridge closed'));
  });
});
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
const RealDate = Date,
  start = Math.floor(Date.now() / 1000) * 1000;
async function atTime(now, work) {
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [now]));
    }
    static now() {
      return now;
    }
  };
  try {
    return await work();
  } finally {
    globalThis.Date = RealDate;
  }
}
const owner = reference.client(),
  peer = reference.client(),
  foreign = reference.client();
const credentials = {
  name: 'First Last',
  firstName: 'First',
  lastName: 'Last',
  email: 'owner@fixture.test',
  password: 'test-only-password',
};
let checks = 0;
const cookie = (who) =>
  [...who.cookies].map(([name, value]) => `${name}=${value}`).join('; ');
async function compareRecords(now) {
  const stored = await native({ op: 'records', now });
  assert.deepEqual(
    stored,
    JSON.parse(
      JSON.stringify({
        users: reference.database.user,
        accounts: reference.database.account,
        sessions: reference.database.session,
      }),
    ),
  );
  checks++;
}
async function compare(
  route,
  { now = start, method = 'GET', headers = {}, body } = {},
) {
  const actual = await atTime(now, () =>
    reference.auth.handler(
      new Request(`http://localhost:3000/api/auth${route}`, {
        method,
        headers,
        ...(body === undefined ? {} : { body }),
      }),
    ),
  );
  const text = await actual.text();
  let actualBody = text;
  try {
    actualBody = JSON.parse(text);
  } catch {}
  const result = await native({
    op: 'request',
    now,
    method,
    path: route,
    headers,
    ...(body === undefined ? {} : { body }),
  });
  assert.equal(
    result.status,
    actual.status,
    JSON.stringify({ route, actualBody, result }),
  );
  if (actual.status < 400) {
    if (route !== '/token') assert.deepEqual(result.body, actualBody, route);
  } else
    assert.deepEqual(
      result.body,
      actualBody,
      JSON.stringify({ route, actualBody, result }),
    );
  checks++;
  for (const name of ['cache-control', 'pragma']) {
    assert.equal(
      result.headers[name]?.[0] ?? null,
      actual.headers.get(name),
      route,
    );
    checks++;
  }
  const actualToken =
    route === '/token' && actual.status === 200
      ? actualBody.token
      : actual.headers.get('set-auth-jwt');
  const nativeToken =
    route === '/token' && result.status === 200
      ? result.body.token
      : result.headers['set-auth-jwt']?.[0];
  if (actualToken) {
    const actualKeys = await atTime(now, async () =>
      (
        await reference.auth.handler(
          new Request('http://localhost:3000/api/auth/jwks'),
        )
      ).json(),
    );
    const nativeKeys = (
      await native({
        op: 'request',
        now,
        method: 'GET',
        path: '/jwks',
        headers: {},
      })
    ).body;
    const options = {
      issuer: 'http://localhost:3000',
      audience: 'http://localhost:3000',
      currentDate: new RealDate(now),
    };
    const actualClaims = await jwtVerify(
      actualToken,
      createLocalJWKSet(actualKeys),
      options,
    );
    const nativeClaims = await jwtVerify(
      nativeToken,
      createLocalJWKSet(nativeKeys),
      options,
    );
    assert.deepEqual(nativeClaims.payload, actualClaims.payload);
    assert.equal(nativeClaims.protectedHeader.alg, 'EdDSA');
    if (route !== '/token')
      assert.ok(
        result.headers['access-control-expose-headers'][0].includes(
          'set-auth-jwt',
        ),
      );
    checks += 2;
  } else assert.equal(nativeToken, undefined);
  await compareRecords(now);
  return { actual, result };
}
try {
  await atTime(start, () => owner.call('/sign-up/email', credentials));
  await atTime(start, () =>
    peer.call('/sign-in/email', {
      email: credentials.email,
      password: credentials.password,
    }),
  );
  await atTime(start, () =>
    foreign.call('/sign-up/email', {
      ...credentials,
      email: 'foreign@fixture.test',
      rememberMe: false,
    }),
  );
  await native({
    op: 'seed',
    now: start,
    users: reference.database.user,
    accounts: reference.database.account,
    sessions: reference.database.session,
  });
  const signed = { cookie: cookie(owner) },
    peerCookie = { cookie: cookie(peer) },
    foreignCookie = { cookie: cookie(foreign) };
  await compare('/ok');
  await compare('/get-session');
  await compare('/get-session', {
    headers: { cookie: 'better-auth.session_token=forged' },
  });
  await compare('/token');
  await compare('/get-session', { headers: signed });
  // JWT token values differ by independent keys, so compare their verified claims.
  // Body equality is handled specially for /token below.
  await compare('/token', { headers: signed });
  await compare('/list-sessions', { headers: signed });
  const hour = start + 3600 * 1000;
  await compare('/get-session?disableRefresh=false', {
    now: hour,
    headers: signed,
  });
  const refreshed = await compare('/get-session', {
    now: hour,
    headers: signed,
  });
  assert.ok(
    refreshed.result.headers['set-cookie'][0].includes('Max-Age=86400'),
  );
  assert.ok(
    refreshed.actual.headers
      .getSetCookie()
      .some((value) => value.includes('Max-Age=86400')),
  );
  checks += 2;
  await compare('/get-session', {
    now: hour,
    method: 'POST',
    headers: { ...signed, origin: 'http://localhost:3000' },
  });
  await compare('/sign-out', {
    now: hour,
    method: 'POST',
    headers: {
      ...signed,
      origin: 'https://evil.test',
      'content-type': 'application/json',
    },
    body: '{}',
  });
  await compare('/sign-out', {
    now: hour,
    method: 'POST',
    headers: { ...signed, 'content-type': 'application/json' },
    body: '{}',
  });
  const revoke = async (target, extra = {}) =>
    compare('/revoke-session', {
      now: hour,
      method: 'POST',
      headers: {
        ...signed,
        origin: 'http://localhost:3000',
        'content-type': 'application/json',
      },
      body: JSON.stringify({ token: target }),
      ...extra,
    });
  await revoke('unknown-token');
  await revoke(
    reference.database.session.find(
      (session) => session.userId === reference.database.user[1].id,
    ).token,
  );
  await revoke(undefined);
  await revoke(123);
  for (const body of ['null', '[]', '{}', '{"token":null}'])
    await compare('/revoke-session', {
      now: hour,
      method: 'POST',
      headers: {
        ...signed,
        origin: 'http://localhost:3000',
        'content-type': 'application/json',
      },
      body,
    });
  await revoke(
    reference.database.session.find(
      (session) =>
        session.token !== reference.database.session[0].token &&
        session.userId === reference.database.user[0].id,
    ).token,
  );
  await compare('/get-session', { now: hour, headers: peerCookie });
  await compare('/get-session', { now: hour, headers: foreignCookie });
  await compare('/revoke-other-sessions', {
    now: hour,
    method: 'POST',
    headers: {
      ...signed,
      origin: 'http://localhost:3000',
      'content-type': 'application/json',
    },
    body: '{}',
  });
  await compare('/sign-out', {
    now: hour,
    method: 'POST',
    headers: {
      ...signed,
      origin: 'http://localhost:3000',
      'content-type': 'application/json',
    },
    body: '{',
  });
  await compare('/sign-out', {
    now: hour,
    method: 'POST',
    headers: {
      ...signed,
      origin: 'http://localhost:3000',
      'content-type': 'text/plain',
    },
    body: '{}',
  });
  for (const body of [
    'null',
    '[]',
    '{"callbackURL":123}',
    '{"disableRedirect":"true"}',
    '{"state":null}',
  ])
    await compare('/sign-out', {
      now: hour,
      method: 'POST',
      headers: {
        ...signed,
        origin: 'http://localhost:3000',
        'content-type': 'application/json',
      },
      body,
    });
  await compare('/revoke-session', {
    now: hour,
    method: 'POST',
    headers: {
      ...signed,
      origin: 'http://localhost:3000',
      'content-type': 'application/json',
    },
  });
  await compare('/revoke-session', {
    now: hour,
    method: 'POST',
    headers: {
      ...signed,
      origin: 'http://localhost:3000',
      'content-type': 'application/x-www-form-urlencoded',
    },
    body: 'token=unknown-token',
  });
  const logout = await compare('/sign-out', {
    now: hour,
    method: 'POST',
    headers: {
      ...signed,
      origin: 'http://localhost:3000',
      'content-type': 'application/json',
    },
    body: '{}',
  });
  assert.equal(logout.result.headers['set-cookie'].length, 4);
  for (const value of logout.result.headers['set-cookie'])
    assert.ok(value.includes('Max-Age=0'));
  checks++;
  await compare('/get-session', { now: hour, headers: signed });
  await compare('/revoke-sessions', {
    now: hour,
    method: 'POST',
    headers: {
      ...foreignCookie,
      origin: 'http://localhost:3000',
      'content-type': 'application/json',
    },
    body: '{}',
  });
  await compare('/get-session', { now: hour, headers: foreignCookie });
  console.log(
    `Better Auth native raw HTTP sessions/JWT: ${checks} actual handler/body/storage/header comparisons passed`,
  );
} finally {
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
  lines.close();
}
