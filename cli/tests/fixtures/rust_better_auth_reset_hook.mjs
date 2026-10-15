import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const root = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const { betterAuthReference } = await import(pathToFileURL(`${root}/cli/tests/fixtures/rust_better_auth_reference.mjs`));
const { betterAuth } = await import(pathToFileURL(`${process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES}/node_modules/better-auth/dist/index.mjs`));
const results = [];
for (const mode of ['success', 'sync-fail', 'async-fail', 'pending', 'background', 'reentry']) {
  const ref = await betterAuthReference(root);
  let token, captured, release, entered;
  let scheduled = 0;
  const enteredPromise = new Promise((resolve) => { entered = resolve; });
  const pending = new Promise((resolve) => { release = resolve; });
  let call;
  const auth = betterAuth({
    ...ref.config,
    advanced: { ...ref.config.advanced, backgroundTasks: { handler: () => { scheduled++; } } },
    emailAndPassword: {
      ...ref.config.emailAndPassword,
      revokeSessionsOnPasswordReset: true,
      sendResetPassword: async (context) => { token = context.token; },
      onPasswordReset: (data, request) => {
        assert.deepEqual(Object.keys(data), ['user']);
        assert.equal(data.user.password, undefined);
        assert.equal(data.user.email, 'hook@example.test');
        assert.equal(data.user.firstName, 'Owned');
        assert.ok(request instanceof Request);
        assert.equal(request.method, 'POST');
        assert.ok(request.url.endsWith('/reset-password'));
        captured = data.user.id;
        if (mode === 'sync-fail') throw Error('owned-sync-failure');
        return (async () => {
          if (mode === 'async-fail') throw Error('owned-async-failure');
          if (mode === 'pending' || mode === 'background') { entered(); await pending; }
          if (mode === 'reentry') {
            const login = await call('/sign-in/email', { email: 'hook@example.test', password: 'new-owned-password' });
            assert.equal(login.status, 200);
          }
        })();
      },
    },
    logger: { disabled: true }, rateLimit: { enabled: false },
  });
  call = (path, body) => auth.handler(new Request(`http://localhost:3000/api/auth${path}`, {
    method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json' }, body: JSON.stringify(body),
  }));
  const signup = await call('/sign-up/email', { name: 'Owned', firstName: 'Owned', lastName: 'User', email: 'hook@example.test', password: 'old-owned-password' });
  assert.equal(signup.status, 200);
  await call('/request-password-reset', { email: 'hook@example.test' });
  assert.ok(token);
  scheduled = 0;
  let settled = false;
  const response = call('/reset-password', { token, newPassword: 'new-owned-password' }).then((value) => { settled = true; return value; });
  if (mode === 'pending' || mode === 'background') {
    await enteredPromise;
    await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal(settled, false);
    assert.equal(ref.database.verification.length, 0);
    assert.equal(ref.database.session.length, 1);
    release();
  }
  const result = await response;
  const failed = mode.endsWith('fail');
  assert.equal(result.status, failed ? 500 : 200);
  assert.ok(captured);
  assert.equal(ref.database.verification.length, 0);
  assert.equal(ref.database.session.length, failed ? 1 : 0);
  assert.equal(scheduled, 0);
  assert.equal(result.headers.has('set-cookie'), false);
  assert.equal((await call('/sign-in/email', { email: 'hook@example.test', password: 'new-owned-password' })).status, 200);
  results.push({ mode, status: result.status, scheduled });
}
console.log('PASS password reset hook actual6', JSON.stringify(results));
