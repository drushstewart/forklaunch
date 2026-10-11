// Native credential records/results compared with actual configured email handlers.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const reference = await betterAuthReference(root);
const useHttp = process.env.FORKLAUNCH_TEST_BETTER_AUTH_NATIVE_HTTP === '1';
const child = spawn(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    'blueprint-rs/Cargo.toml',
    '-p',
    'forklaunch-iam-better-auth',
    ...(useHttp ? ['--features', 'http'] : []),
    '--example',
    useHttp ? 'http_parity' : 'accounts_parity',
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
    else reject(new Error(`Credential fixture exited ${code}`));
    for (const next of pending.splice(0))
      next.reject(new Error('Native bridge closed'));
  });
});
completion.catch(() => {});
const exchange = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
const browserIds = new WeakMap(),
  browsers = new Map();
let nextBrowser = 0;
const browserId = (who) => {
  if (!browserIds.has(who)) browserIds.set(who, ++nextBrowser);
  return browserIds.get(who);
};
async function native(input) {
  if (!useHttp || input.op === 'records' || input.op === 'request')
    return exchange(input);
  const routes = {
    signup: '/sign-up/email',
    signin: '/sign-in/email',
    get: '/get-session',
    verify: '/verify-password',
    change: '/change-password',
  };
  const jar = browsers.get(input.client) ?? new Map();
  browsers.set(input.client, jar);
  let cookie = [...jar].map(([name, value]) => `${name}=${value}`).join('; ');
  if (input.client === undefined && input.token) {
    const sign = (value) =>
      encodeURIComponent(
        `${value}.${createHmac('sha256', 'better-auth-protocol-fixture-secret-32-characters').update(value).digest('base64')}`,
      );
    cookie = `better-auth.session_token=${sign(input.token)}`;
    if (input.transient)
      cookie += `; better-auth.dont_remember=${sign('true')}`;
  }
  const result = await exchange({
    op: 'request',
    now: input.now,
    method: input.op === 'get' ? 'GET' : 'POST',
    path: routes[input.op],
    headers: {
      cookie,
      origin: 'http://localhost:3000',
      'content-type': 'application/json',
      ...input.headers,
    },
    ...(input.op === 'get' ? {} : { body: JSON.stringify(input.payload) }),
  });
  for (const header of result.headers['set-cookie'] ?? []) {
    const pair = header.split(';')[0],
      separator = pair.indexOf('=');
    const name = pair.slice(0, separator),
      value = pair.slice(separator + 1);
    if (value) jar.set(name, value);
    else jar.delete(name);
  }
  if (result.status >= 400)
    return {
      error: result.body.code,
      status: result.status,
      headers: result.headers,
    };
  return {
    body: result.body,
    headers: result.headers,
    status: result.status,
    persistent:
      result.headers['set-cookie']
        ?.find((cookie) => cookie.startsWith('better-auth.session_token='))
        ?.includes('Max-Age=86400') ?? false,
  };
}
const RealDate = Date,
  start = Math.floor(RealDate.now() / 1000) * 1000;
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
const actualIds = new Map(),
  nativeIds = new Map();
let login = 0,
  checks = 0;
function normalize(value, ids) {
  if (typeof value === 'string') return ids.get(value) ?? value;
  if (Array.isArray(value)) return value.map((item) => normalize(item, ids));
  if (value && typeof value === 'object')
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [
        key,
        key === 'password' ? '[stored hash]' : normalize(item, ids),
      ]),
    );
  return value;
}
async function compareRecords(now) {
  const stored = await native({ op: 'records', now });
  const actual = {
    users: reference.database.user,
    accounts: reference.database.account,
    sessions: reference.database.session,
  };
  for (const [source, ids] of [
    [actual, actualIds],
    [stored, nativeIds],
  ]) {
    for (const row of source.accounts) {
      ids.set(row.id, `account:${ids.get(row.userId)}:${row.providerId}`);
      assert.match(row.password, /^[a-f0-9]{32}:[a-f0-9]{128}$/);
    }
    for (const row of source.sessions)
      ids.set(row.id, `session-record:${ids.get(row.token)}`);
  }
  assert.deepEqual(
    normalize(stored, nativeIds),
    normalize(JSON.parse(JSON.stringify(actual)), actualIds),
  );
  checks++;
}
async function authenticate(
  who,
  now,
  op,
  payload,
  status = 200,
  code,
  headers = {},
) {
  const route = op === 'signup' ? '/sign-up/email' : '/sign-in/email';
  const actual = await atTime(now, () => who.call(route, payload, { headers }));
  assert.equal(actual.status, status, JSON.stringify(actual.body));
  const result = await native({
    op,
    now,
    payload,
    client: browserId(who),
    headers,
  });
  if (useHttp) assert.equal(result.status, actual.status);
  if (status === 200) {
    const user = `user:${actual.body.user.email}`;
    actualIds.set(actual.body.user.id, user);
    nativeIds.set(result.body.user.id, user);
    const session = `login:${++login}`;
    actualIds.set(actual.body.token, session);
    nativeIds.set(result.body.token, session);
    assert.deepEqual(
      normalize(result.body, nativeIds),
      normalize(actual.body, actualIds),
    );
    assert.equal(result.persistent, payload.rememberMe !== false);
    const cookie = actual.headers
      .getSetCookie()
      .find((value) => value.startsWith('better-auth.session_token='));
    assert.equal(cookie.includes('Max-Age=86400'), result.persistent);
    if (useHttp) {
      const attributes = (cookies) =>
        cookies
          .map((cookie) => [
            cookie.split('=')[0],
            cookie.slice(cookie.indexOf(';')),
          ])
          .sort();
      assert.deepEqual(
        attributes(result.headers['set-cookie']),
        attributes(actual.headers.getSetCookie()),
      );
      assert.equal(
        result.headers.location?.[0] ?? null,
        actual.headers.get('location'),
      );
      checks += 2;
    }

    const current = await atTime(now, () => who.call('/get-session'));
    const nativeCurrent = await native({
      op: 'get',
      now,
      token: result.body.token,
      transient: !result.persistent,
      client: browserId(who),
    });
    actualIds.set(current.body.session.id, `session-record:${session}`);
    nativeIds.set(nativeCurrent.body.session.id, `session-record:${session}`);
    assert.deepEqual(
      normalize(nativeCurrent.body, nativeIds),
      normalize(current.body, actualIds),
    );
    checks += 4;
  } else {
    assert.equal(actual.body.code, code);
    assert.equal(result.error, code);
    checks++;
  }
  await compareRecords(now);
  return result;
}
const owner = reference.client();
const signup = {
  name: 'First Last',
  firstName: 'First',
  lastName: 'Last',
  email: 'Owner@Example.test',
  password: 'test-only-password',
};
try {
  const ownerGrant = await authenticate(owner, start, 'signup', signup);
  let ownerToken = ownerGrant.body.token;
  await authenticate(
    reference.client(),
    start + 1000,
    'signup',
    signup,
    422,
    'USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL',
  );
  await authenticate(
    reference.client(),
    start + 2000,
    'signup',
    { ...signup, email: 'short@example.test', password: 'short' },
    400,
    'PASSWORD_TOO_SHORT',
  );
  await authenticate(
    reference.client(),
    start + 3000,
    'signup',
    { ...signup, email: 'long@example.test', password: 'x'.repeat(129) },
    400,
    'PASSWORD_TOO_LONG',
  );
  await authenticate(
    reference.client(),
    start + 4000,
    'signin',
    { email: 'owner@example.test', password: 'incorrect' },
    401,
    'INVALID_EMAIL_OR_PASSWORD',
  );
  await authenticate(
    reference.client(),
    start + 5000,
    'signin',
    { email: 'absent@example.test', password: 'test-only-password' },
    401,
    'INVALID_EMAIL_OR_PASSWORD',
  );
  await authenticate(
    reference.client(),
    start + 6000,
    'signin',
    { email: 'invalid-email', password: 'test-only-password' },
    400,
    'INVALID_EMAIL',
  );
  await authenticate(
    reference.client(),
    start + 7000,
    'signin',
    { email: 'owner@example.test', password: 'x'.repeat(129) },
    400,
    'PASSWORD_TOO_LONG',
  );
  await authenticate(reference.client(), start + 8000, 'signin', {
    email: 'OWNER@EXAMPLE.TEST',
    password: 'test-only-password',
    rememberMe: false,
    callbackURL: '',
  });
  await authenticate(reference.client(), start + 9000, 'signin', {
    email: 'owner@example.test',
    password: 'test-only-password',
    callbackURL: '/dashboard',
  });
  const unicode = {
    ...signup,
    email: 'unicode@example.test',
    password: '\ud83d\udd10'.repeat(4),
    firstName: '\u00c5',
    lastName: '',
    phoneNumber: '+15555550123',
    image: 'https://fixture.test/avatar.png',
  };
  await authenticate(reference.client(), start + 10000, 'signup', unicode);
  await authenticate(reference.client(), start + 11000, 'signin', {
    email: unicode.email,
    password: unicode.password,
  });
  async function passwordOperation(op, payload, offset, status = 200, code) {
    const now = start + offset;
    const actual = await atTime(now, () =>
      owner.call(
        op === 'change' ? '/change-password' : '/verify-password',
        payload,
      ),
    );
    assert.equal(actual.status, status, JSON.stringify(actual.body));
    const result = await native({
      op,
      payload,
      token: ownerToken,
      now,
      client: browserId(owner),
    });
    if (status === 200) {
      if (actual.body.token) {
        const session = `login:${++login}`;
        actualIds.set(actual.body.token, session);
        nativeIds.set(result.body.token, session);
        ownerToken = result.body.token;
        assert.ok(
          actual.headers
            .getSetCookie()
            .some(
              (cookie) =>
                cookie.startsWith('better-auth.session_token=') &&
                cookie.includes('Max-Age=86400'),
            ),
        );
        checks++;
      }
      assert.deepEqual(
        normalize(result.body, nativeIds),
        normalize(actual.body, actualIds),
      );
    } else {
      assert.equal(actual.body.code, code);
      assert.equal(result.error, code);
    }
    checks++;
    await compareRecords(now);
    return result;
  }
  await passwordOperation('verify', { password: signup.password }, 12000);
  await passwordOperation(
    'verify',
    { password: 'incorrect' },
    13000,
    400,
    'INVALID_PASSWORD',
  );
  await passwordOperation(
    'verify',
    { password: 'x'.repeat(129) },
    14000,
    400,
    'PASSWORD_TOO_LONG',
  );
  await passwordOperation(
    'change',
    { currentPassword: signup.password, newPassword: 'short' },
    15000,
    400,
    'PASSWORD_TOO_SHORT',
  );
  await passwordOperation(
    'change',
    { currentPassword: 'incorrect', newPassword: 'new-test-only-password' },
    16000,
    400,
    'INVALID_PASSWORD',
  );
  await passwordOperation(
    'change',
    { currentPassword: signup.password, newPassword: 'x'.repeat(129) },
    17000,
    400,
    'PASSWORD_TOO_LONG',
  );
  await passwordOperation(
    'change',
    { currentPassword: 'x'.repeat(129), newPassword: 'new-test-only-password' },
    18000,
    400,
    'PASSWORD_TOO_LONG',
  );
  await passwordOperation(
    'change',
    { currentPassword: signup.password, newPassword: 'new-test-only-password' },
    19000,
  );
  await passwordOperation(
    'verify',
    { password: signup.password },
    20000,
    400,
    'INVALID_PASSWORD',
  );
  await passwordOperation(
    'verify',
    { password: 'new-test-only-password' },
    21000,
  );
  await authenticate(
    reference.client(),
    start + 22000,
    'signin',
    { email: signup.email, password: signup.password },
    401,
    'INVALID_EMAIL_OR_PASSWORD',
  );
  await authenticate(reference.client(), start + 23000, 'signin', {
    email: signup.email,
    password: 'new-test-only-password',
  });
  const original = ownerToken;
  const foreignSessions = reference.database.session
    .filter((session) => session.userId !== reference.database.user[0].id)
    .map((session) => session.id);
  await passwordOperation(
    'change',
    {
      currentPassword: 'new-test-only-password',
      newPassword: 'third-test-only-password',
      revokeOtherSessions: true,
    },
    24000,
  );
  assert.equal(
    reference.database.session.filter(
      (session) => session.userId === reference.database.user[0].id,
    ).length,
    1,
  );
  assert.deepEqual(
    reference.database.session
      .filter((session) => session.userId !== reference.database.user[0].id)
      .map((session) => session.id),
    foreignSessions,
  );
  assert.equal(
    (await native({ op: 'get', token: original, now: start + 25000 })).body,
    null,
  );
  checks += 3;
  await passwordOperation(
    'verify',
    { password: 'third-test-only-password' },
    26000,
  );
  await authenticate(
    reference.client(),
    start + 27000,
    'signin',
    { email: signup.email, password: 'new-test-only-password' },
    401,
    'INVALID_EMAIL_OR_PASSWORD',
  );
  await authenticate(reference.client(), start + 28000, 'signin', {
    email: signup.email,
    password: 'third-test-only-password',
  });
  if (useHttp) {
    await authenticate(
      reference.client(),
      start + 29000,
      'signup',
      {
        ...signup,
        email: 'opaque-profile@fixture.test',
        firstName: 123,
        lastName: null,
        phoneNumber: null,
      },
      200,
      undefined,
      {
        'x-forwarded-for': '2001:db8:1:2:3:4:5:6',
        'user-agent': 'native-profile-fixture',
      },
    );
    await authenticate(
      reference.client(),
      start + 30000,
      'signup',
      {
        ...signup,
        email: 'ignored-private@fixture.test',
        id: 'forged-user-id',
        emailVerified: true,
        role: 'system',
        createdAt: '2000-01-01T00:00:00Z',
        privateSecret: 'ignored-secret',
      },
      200,
      undefined,
      {
        'x-forwarded-for': '192.0.2.1, 198.51.100.2',
        'user-agent': 'native-chain-fixture',
      },
    );
    const transient = reference.client();
    const grant = await authenticate(
      transient,
      start + 31000,
      'signin',
      {
        email: unicode.email,
        password: unicode.password,
        rememberMe: false,
      },
      200,
      undefined,
      {
        'x-forwarded-for': '::ffff:192.0.2.15',
        'user-agent': 'native-mapped-fixture',
      },
    );
    const payload = {
      currentPassword: unicode.password,
      newPassword: 'replacement-unicode-password',
      revokeOtherSessions: true,
    };
    const actual = await atTime(start + 32000, () =>
      transient.call('/change-password', payload),
    );
    const result = await native({
      op: 'change',
      payload,
      now: start + 32000,
      token: grant.body.token,
      client: browserId(transient),
    });
    assert.equal(actual.status, 200, JSON.stringify(actual.body));
    assert.equal(result.status, 200);
    const session = `login:${++login}`;
    actualIds.set(actual.body.token, session);
    nativeIds.set(result.body.token, session);
    assert.deepEqual(
      normalize(result.body, nativeIds),
      normalize(actual.body, actualIds),
    );
    const attributes = (cookies) =>
      cookies
        .map((cookie) => [
          cookie.split('=')[0],
          cookie.slice(cookie.indexOf(';')),
        ])
        .sort();
    assert.deepEqual(
      attributes(result.headers['set-cookie']),
      attributes(actual.headers.getSetCookie()),
    );
    assert.ok(
      result.headers['set-cookie'].some((cookie) =>
        cookie.startsWith('better-auth.dont_remember='),
      ),
    );
    assert.ok(
      result.headers['set-cookie'].every(
        (cookie) => !cookie.includes('Max-Age='),
      ),
    );
    checks += 5;
    await compareRecords(start + 32000);
  }
  console.log(
    `Better Auth native ${useHttp ? 'HTTP' : 'backend'} email credentials: ${checks} body/storage/protocol comparisons passed`,
  );
} finally {
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
  lines.close();
}
