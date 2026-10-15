import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const root = new URL('../../..', import.meta.url).pathname.replace(/\/$/, '');
const dist = `${process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES}/node_modules/better-auth/dist/`;
const { betterAuthReference } = await import(pathToFileURL(`${root}/cli/tests/fixtures/rust_better_auth_reference.mjs`));
const { betterAuth } = await import(pathToFileURL(`${dist}index.mjs`));
const { createEmailVerificationToken } = await import(pathToFileURL(`${dist}api/routes/email-verification.mjs`));
const rows = [];
for (const mode of ['ordinary', 'already', 'before-sync', 'before-async', 'after-async', 'before-pending', 'after-pending', 'change', 'change-after-fail', 'legacy', 'confirmation', 'redirect-before-fail']) {
  const ref = await betterAuthReference(root);
  let before = 0, after = 0, scheduled = 0, release, entered;
  const enteredPromise = new Promise((resolve) => { entered = resolve; });
  const pending = new Promise((resolve) => { release = resolve; });
  const hook = (phase, user, request) => {
    assert.equal(user.user, undefined);
    assert.equal(user.password, undefined);
    assert.equal(user.firstName, 'Owned');
    assert.ok(request instanceof Request);
    assert.equal(request.method, 'GET');
    assert.equal(user.emailVerified, phase === 'after');
    if (phase === 'before') before++; else after++;
    if (phase === 'before' && mode === 'before-sync') throw Error('owned-before');
    return (async () => {
      if ((phase === 'before' && ['before-async', 'redirect-before-fail'].includes(mode)) ||
          (phase === 'after' && ['after-async', 'change-after-fail'].includes(mode))) throw Error('owned-after');
      if (mode === `${phase}-pending`) { entered(); await pending; }
    })();
  };
  const auth = betterAuth({ ...ref.config,
    advanced: { ...ref.config.advanced, backgroundTasks: { handler: () => { scheduled++; } } },
    emailVerification: { beforeEmailVerification: (user, request) => hook('before', user, request), afterEmailVerification: (user, request) => hook('after', user, request) },
    logger: { disabled: true }, rateLimit: { enabled: false },
  });
  await auth.handler(new Request('http://localhost:3000/api/auth/sign-up/email', { method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json' }, body: JSON.stringify({ name: 'Owned', firstName: 'Owned', lastName: 'User', email: 'verify-hook@example.test', password: 'owned-hook-password' }) }));
  if (mode === 'already') ref.database.user[0].emailVerified = true;
  const changing = ['change', 'change-after-fail', 'legacy', 'confirmation'].includes(mode);
  const requestType = mode === 'confirmation' ? 'change-email-confirmation' : mode === 'legacy' ? undefined : 'change-email-verification';
  const token = await createEmailVerificationToken((await auth.$context).secret, 'verify-hook@example.test', changing ? 'changed@example.test' : undefined, undefined, changing && requestType ? { requestType } : undefined);
  let settled = false;
  const response = auth.handler(new Request(`http://localhost:3000/api/auth/verify-email?token=${token}${mode === 'redirect-before-fail' ? '&callbackURL=%2Fdone' : ''}`)).then((value) => { settled = true; return value; });
  if (mode.endsWith('pending')) {
    await enteredPromise;
    await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal(settled, false);
    assert.equal(ref.database.user[0].emailVerified, mode === 'after-pending');
    release();
  }
  const result = await response;
  const failed = ['before-sync', 'before-async', 'after-async', 'change-after-fail', 'redirect-before-fail'].includes(mode);
  assert.equal(result.status, failed ? 500 : 200);
  const skipped = ['already', 'legacy', 'confirmation'].includes(mode);
  assert.equal(before, changing || skipped ? 0 : 1);
  assert.equal(after, skipped || ['before-sync', 'before-async', 'redirect-before-fail'].includes(mode) ? 0 : 1);
  assert.equal(ref.database.user[0].emailVerified, !['before-sync', 'before-async', 'redirect-before-fail', 'legacy', 'confirmation'].includes(mode));
  assert.equal(scheduled, 0);
  assert.equal(result.headers.has('set-cookie'), changing && !failed && mode !== 'confirmation');
  assert.equal(ref.database.session.length, changing && mode !== 'confirmation' ? 2 : 1);
  if (failed) assert.equal(result.headers.has('location'), false);
  rows.push({ mode, status: result.status, before, after, verified: ref.database.user[0].emailVerified, sessions: ref.database.session.length });
}
console.log('PASS email verification hooks actual12', JSON.stringify(rows));
