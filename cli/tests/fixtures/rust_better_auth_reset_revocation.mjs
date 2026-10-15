import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';

const root = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const { betterAuthReference } = await import(
  pathToFileURL(`${root}/cli/tests/fixtures/rust_better_auth_reference.mjs`)
);
const { betterAuth } = await import(
  pathToFileURL(`${process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES}/node_modules/better-auth/dist/index.mjs`)
);
const rows = [];
for (const mode of ['default', 'revoke', 'hook-fail', 'delete-fail']) {
  const ref = await betterAuthReference(root);
  let token;
  let hookCalls = 0;
  const auth = betterAuth({
    ...ref.config,
    emailAndPassword: {
      ...ref.config.emailAndPassword,
      ...(mode === 'default' ? {} : { revokeSessionsOnPasswordReset: true }),
      sendResetPassword: async (context) => { token = context.token; },
      onPasswordReset: async () => {
        hookCalls++;
        if (mode === 'hook-fail') throw Error('owned-reset-hook');
      },
    },
    logger: { disabled: true },
    rateLimit: { enabled: false },
  });
  const call = (route, body) => auth.handler(new Request(`http://localhost:3000/api/auth${route}`, {
    method: 'POST',
    headers: { origin: 'http://localhost:3000', 'content-type': 'application/json' },
    body: JSON.stringify(body),
  }));
  const input = {
    name: 'Owned', firstName: 'Owned', lastName: 'User',
    email: 'revoke@example.test', password: 'owned-reset-password',
  };
  const first = await (await call('/sign-up/email', input)).json();
  await call('/sign-in/email', input);
  await call('/sign-up/email', { ...input, email: 'foreign@example.test' });
  ref.database.session.push({
    ...ref.database.session.find((session) => session.userId === first.user.id),
    id: crypto.randomUUID(), token: 'owned-expired', expiresAt: new Date(0),
  });
  const before = ref.database.session.filter((session) => session.userId === first.user.id).length;
  await call('/request-password-reset', { email: input.email });
  assert.ok(token);
  if (mode === 'delete-fail') {
    const context = await auth.$context;
    context.internalAdapter.deleteUserSessions = async () => { throw Error('owned-delete'); };
  }
  const changed = await call('/reset-password', { token, newPassword: 'owned-new-password' });
  const own = ref.database.session.filter((session) => session.userId === first.user.id).length;
  const foreign = ref.database.session.filter((session) => session.userId !== first.user.id).length;
  assert.equal(changed.status, ['hook-fail', 'delete-fail'].includes(mode) ? 500 : 200);
  assert.equal(own, mode === 'revoke' ? 0 : before);
  assert.equal(foreign, 1);
  assert.equal(ref.database.verification.length, 0);
  assert.equal(changed.headers.has('set-cookie'), false);
  const signin = await call('/sign-in/email', { email: input.email, password: 'owned-new-password' });
  assert.equal(signin.status, 200);
  rows.push({ mode, status: changed.status, before, remaining: own, foreign, hookCalls, newPasswordAccepted: signin.status });
}
console.log('PASS reset revocation actual4', JSON.stringify(rows));
