// Actual configured session refresh, freshness and revocation semantics.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const reference = await betterAuthReference(root);
const RealDate = Date;
const start = Math.floor(RealDate.now() / 1000) * 1000;
const hour = 3600 * 1000,
  day = 24 * hour;
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
let checks = 0;
async function call(who, now, route, body, status = 200, code, request = {}) {
  const response = await atTime(now, () => who.call(route, body, request));
  assert.equal(
    response.status,
    status,
    `${route}: ${JSON.stringify(response.body)}`,
  );
  if (code) assert.equal(response.body.code, code);
  checks++;
  return response;
}
const signup = (email, rememberMe = true) => ({
  name: 'Session Person',
  firstName: 'Session',
  lastName: 'Person',
  email,
  password: 'test-only-password',
  rememberMe,
});
const credentials = {
  email: 'sessions@example.test',
  password: 'test-only-password',
};
try {
  const owner = reference.client(),
    peer = reference.client(),
    other = reference.client();
  const created = await call(
    owner,
    start,
    '/sign-up/email',
    signup(credentials.email),
  );
  const initial = await call(owner, start, '/get-session');
  assert.equal(
    new RealDate(initial.body.session.expiresAt).getTime(),
    start + day,
  );
  assert.equal(initial.headers.get('cache-control'), 'no-store');
  assert.equal(initial.headers.get('pragma'), 'no-cache');
  const peerLogin = await call(
    peer,
    start + 1000,
    '/sign-in/email',
    credentials,
  );
  const otherLogin = await call(
    other,
    start + 2000,
    '/sign-up/email',
    signup('other-sessions@example.test'),
  );
  const beforeRefresh = await call(owner, start + hour - 1, '/get-session');
  assert.equal(
    beforeRefresh.body.session.expiresAt,
    initial.body.session.expiresAt,
  );
  const refreshed = await call(owner, start + hour, '/get-session');
  assert.equal(
    new RealDate(refreshed.body.session.expiresAt).getTime(),
    start + hour + day,
  );
  assert.equal(
    new RealDate(refreshed.body.session.updatedAt).getTime(),
    start + hour,
  );
  assert.equal(
    refreshed.body.session.createdAt,
    initial.body.session.createdAt,
  );
  assert.equal(refreshed.body.session.token, created.body.token);
  assert.ok(
    refreshed.headers
      .getSetCookie()
      .some(
        (cookie) =>
          cookie.startsWith('better-auth.session_token=') &&
          cookie.includes('Max-Age=86400'),
      ),
  );
  const skipped = await call(
    owner,
    start + 2 * hour,
    '/get-session?disableRefresh=true',
  );
  assert.equal(
    skipped.body.session.expiresAt,
    refreshed.body.session.expiresAt,
  );
  await call(
    owner,
    start + 2 * hour,
    '/get-session',
    {},
    405,
    'METHOD_NOT_ALLOWED_DEFER_SESSION_REQUIRED',
  );
  const listed = await call(owner, start + 2 * hour, '/list-sessions');
  assert.equal(listed.body.length, 2);
  assert.ok(
    listed.body.every((session) => session.userId === created.body.user.id),
  );
  // Session freshness is based on creation, even when hourly refresh keeps it valid.
  await call(
    owner,
    start + day,
    '/list-sessions',
    undefined,
    403,
    'SESSION_NOT_FRESH',
  );
  const stillValid = await call(owner, start + day, '/get-session');
  assert.equal(stillValid.body.user.id, created.body.user.id);
  const freshPeer = reference.client();
  await call(freshPeer, start + day, '/sign-in/email', credentials);
  const freshList = await call(freshPeer, start + day, '/list-sessions');
  assert.equal(freshList.body.length, 3);
  // Sensitive revocation re-reads authoritative storage, but does not require freshness.
  assert.deepEqual(
    (
      await call(owner, start + day, '/revoke-session', {
        token: 'missing-token',
      })
    ).body,
    { status: true },
  );
  assert.deepEqual(
    (
      await call(owner, start + day, '/revoke-session', {
        token: otherLogin.body.token,
      })
    ).body,
    { status: true },
  );
  assert.equal(
    (await call(other, start + day, '/get-session')).body.user.id,
    otherLogin.body.user.id,
  );
  await call(owner, start + day, '/revoke-session', {
    token: peerLogin.body.token,
  });
  assert.equal((await call(peer, start + day, '/get-session')).body, null);
  await call(owner, start + day, '/revoke-other-sessions', {});
  assert.equal((await call(freshPeer, start + day, '/get-session')).body, null);
  assert.equal(
    (await call(owner, start + day, '/get-session')).body.user.id,
    created.body.user.id,
  );
  const finalPeer = reference.client();
  await call(finalPeer, start + day, '/sign-in/email', credentials);
  const captured = [...owner.cookies];
  await call(owner, start + day, '/revoke-sessions', {});
  assert.equal((await call(owner, start + day, '/get-session')).body, null);
  assert.equal((await call(finalPeer, start + day, '/get-session')).body, null);
  const replay = reference.client();
  for (const [name, value] of captured) replay.cookies.set(name, value);
  assert.equal((await call(replay, start + day, '/get-session')).body, null);
  await call(
    replay,
    start + day,
    '/list-sessions',
    undefined,
    401,
    'UNAUTHORIZED',
  );
  // Transient cookies still have a persisted one-day session, without refresh.
  const transient = reference.client(),
    transientStart = start + 2 * day;
  const transientLogin = await call(
    transient,
    transientStart,
    '/sign-up/email',
    signup('transient@example.test', false),
  );
  const sessionCookie = transientLogin.headers
    .getSetCookie()
    .find((cookie) => cookie.startsWith('better-auth.session_token='));
  assert.ok(sessionCookie);
  assert.ok(!sessionCookie.includes('Max-Age='));
  assert.ok(transient.cookies.has('better-auth.dont_remember'));
  const transientSession = await call(
    transient,
    transientStart + hour,
    '/get-session',
  );
  assert.equal(
    new RealDate(transientSession.body.session.expiresAt).getTime(),
    transientStart + day,
  );
  assert.equal(
    new RealDate(transientSession.body.session.updatedAt).getTime(),
    transientStart,
  );
  // Actual get-session uses strictly-less-than expiry; the next millisecond expires it.
  assert.equal(
    (await call(transient, transientStart + day, '/get-session')).body.user.id,
    transientLogin.body.user.id,
  );
  assert.equal(
    (await call(transient, transientStart + day + 1, '/get-session')).body,
    null,
  );
  assert.equal(
    reference.database.session.some(
      (session) => session.token === transientLogin.body.token,
    ),
    false,
  );
  assert.equal(transient.cookies.has('better-auth.session_token'), false);
  assert.equal(transient.cookies.has('better-auth.dont_remember'), false);
  checks += 29;
  console.log(
    `Configured Better Auth session lifecycle: ${checks} checks passed`,
  );
} finally {
  globalThis.Date = RealDate;
}
