// Native credential records/results compared with actual configured email handlers.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const reference = await betterAuthReference(root);
const child = spawn(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    'blueprint-rs/Cargo.toml',
    '-p',
    'forklaunch-iam-better-auth',
    '--example',
    'accounts_parity',
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
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
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
async function authenticate(who, now, op, payload, status = 200, code) {
  const route = op === 'signup' ? '/sign-up/email' : '/sign-in/email';
  const actual = await atTime(now, () => who.call(route, payload));
  assert.equal(actual.status, status, JSON.stringify(actual.body));
  const result = await native({ op, now, payload });
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
    const current = await atTime(now, () => who.call('/get-session'));
    const nativeCurrent = await native({
      op: 'get',
      now,
      token: result.body.token,
      transient: !result.persistent,
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
  await authenticate(owner, start, 'signup', signup);
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
  console.log(
    `Better Auth native email credentials: ${checks} body/storage/protocol comparisons passed`,
  );
} finally {
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
  lines.close();
}
