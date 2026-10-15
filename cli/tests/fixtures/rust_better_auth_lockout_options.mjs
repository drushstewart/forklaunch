import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const { betterAuthReference } = await import(pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const p = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/better-auth/dist/';
const { betterAuth } = await import(pathToFileURL(p + 'index.mjs'));
const { twoFactor } = await import(pathToFileURL(p + 'plugins/two-factor/index.mjs'));
const RealDate = Date;
let clock = 1800000000000;
globalThis.Date = class extends RealDate {
    constructor(...a) { super(...(a.length ? a : [clock])); }
    static now() { return clock; }
    static [Symbol.hasInstance](x) { return x instanceof RealDate; }
};
try {
    const { recordTwoFactorFailure } = await import(pathToFileURL(p + 'plugins/two-factor/verify-two-factor.mjs'));
    for (const now of [0, -1, -1001, 1800000000000])
        for (const duration of [0, 1, 1001, 2007]) {
            clock = now;
            let deadline;
            const ctx = { context: { getPlugin: () => ({ options: { accountLockout: { maxFailedAttempts: 1, durationSeconds: duration / 1000 } } }), adapter: { incrementOne: async (input) => { if (input.set)
                            deadline = +input.set.lockedUntil; return { failedVerificationCount: 1 }; } } } };
            await recordTwoFactorFailure(ctx, 'twoFactor', { id: 'clock-probe' });
            assert.equal(deadline, new RealDate(now + (duration / 1000) * 1000).getTime());
            if (now === 0 && duration === 1001)
                assert.equal(deadline, 1000);
            if (now === -1001 && duration === 1001)
                assert.equal(Object.is(deadline, -0), false);
        }
    clock = 1800000000000;
    const ref = await betterAuthReference(root);
    ref.database.twoFactor = [];
    let auth, otp;
    let denyFactorWrites = false, blockedWrites = 0;
    const jar = new Map();
    function configure(policy) { auth = betterAuth({ ...ref.config, database: (opts) => { const adapter = ref.config.database(opts); const guarded = (method) => (input) => { if (denyFactorWrites && input.model === 'twoFactor') {
            blockedWrites++;
            throw new Error('owned factor update denied');
        } return adapter[method](input); }; return { ...adapter, update: guarded('update'), incrementOne: guarded('incrementOne') }; }, plugins: [...ref.config.plugins, twoFactor({ skipVerificationOnEnable: true, accountLockout: policy, otpOptions: { sendOTP: ({ otp: code }) => { otp = code; } } })], logger: { disabled: true }, rateLimit: { enabled: false } }); }
    async function call(route, body = {}) { const r = await auth.handler(new Request('http://localhost:3000/api/auth' + route, { method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json', cookie: [...jar].map(([k, v]) => k + '=' + v).join('; ') }, body: JSON.stringify(body) })); for (const c of r.headers.getSetCookie()) {
        const pair = c.split(';')[0], i = pair.indexOf('=');
        if (c.includes('Max-Age=0'))
            jar.delete(pair.slice(0, i));
        else
            jar.set(pair.slice(0, i), pair.slice(i + 1));
    } const text = await r.text(); return { status: r.status, body: text ? JSON.parse(text) : null }; }
    const creds = { name: 'Lock', firstName: 'Lock', lastName: 'Test', email: 'lock@example.test', password: 'owned-factor-password' };
    configure({ maxFailedAttempts: 2, durationSeconds: 1.5 });
    await call('/sign-up/email', creds);
    const enrolled = await call('/two-factor/enable', { password: creds.password });
    const authenticated = new Map(jar);
    jar.clear();
    await call('/sign-in/email', creds);
    assert.equal((await call('/two-factor/verify-totp', { code: 'invalid' })).body.code, 'INVALID_CODE');
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 1);
    assert.equal((await call('/two-factor/verify-backup-code', { code: 'invalid' })).body.code, 'INVALID_BACKUP_CODE');
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 2);
    assert.equal(+ref.database.twoFactor[0].lockedUntil, clock + 1500);
    await call('/two-factor/send-otp');
    assert.ok(otp);
    assert.equal((await call('/two-factor/verify-otp', { code: otp })).status, 429);
    assert.equal((await call('/two-factor/verify-backup-code', { code: enrolled.body.backupCodes[0] })).status, 429);
    const pending = new Map(jar);
    jar.clear();
    for (const x of authenticated)
        jar.set(...x);
    assert.equal((await call('/two-factor/verify-backup-code', { code: enrolled.body.backupCodes[0] })).status, 200);
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 2);
    jar.clear();
    for (const x of pending)
        jar.set(...x);
    configure({ enabled: false, maxFailedAttempts: 1, durationSeconds: 10 });
    assert.equal((await call('/two-factor/verify-backup-code', { code: 'invalid' })).body.code, 'INVALID_BACKUP_CODE');
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 2);
    assert.equal((await call('/two-factor/verify-otp', { code: otp })).status, 200);
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 2);
    assert.equal(+ref.database.twoFactor[0].lockedUntil, clock + 1500);
    jar.clear();
    await call('/sign-in/email', creds);
    configure({ enabled: true, maxFailedAttempts: 5, durationSeconds: 20 });
    clock += 1499;
    assert.equal((await call('/two-factor/verify-backup-code', { code: 'invalid' })).status, 429);
    clock += 1;
    assert.equal((await call('/two-factor/verify-backup-code', { code: 'invalid' })).body.code, 'INVALID_BACKUP_CODE');
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 1);
    assert.equal(ref.database.twoFactor[0].lockedUntil, null);
    configure({ maxFailedAttempts: 1, durationSeconds: 0 });
    assert.equal((await call('/two-factor/verify-backup-code', { code: 'invalid' })).body.code, 'INVALID_BACKUP_CODE');
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 2);
    assert.equal(+ref.database.twoFactor[0].lockedUntil, clock);
    assert.equal((await call('/two-factor/verify-backup-code', { code: enrolled.body.backupCodes[1] })).status, 200);
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 0);
    jar.clear();
    configure({ enabled: false });
    await call('/sign-in/email', creds);
    for (let i = 0; i < 5; i++)
        assert.equal((await call('/two-factor/verify-backup-code', { code: 'invalid' })).body.code, 'INVALID_BACKUP_CODE');
    ref.database.twoFactor[0].failedVerificationCount = 4;
    ref.database.twoFactor[0].lockedUntil = new Date(clock);
    configure({ enabled: true });
    assert.equal((await call('/two-factor/verify-backup-code', { code: 'invalid' })).body.code, 'TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE');
    assert.equal(ref.database.twoFactor[0].failedVerificationCount, 0);
    assert.equal(ref.database.twoFactor[0].lockedUntil, null);
    jar.clear();
    configure({ maxFailedAttempts: 1, durationSeconds: 10 });
    const only = { ...creds, email: 'otp-only@example.test' };
    await call('/sign-up/email', only);
    assert.equal((await call('/two-factor/enable', { method: 'otp', password: only.password })).status, 200);
    jar.clear();
    await call('/sign-in/email', only);
    await call('/two-factor/send-otp');
    for (let i = 0; i < 2; i++)
        assert.equal((await call('/two-factor/verify-otp', { code: 'invalid' })).body.code, 'INVALID_CODE');
    assert.equal((await call('/two-factor/verify-otp', { code: otp })).status, 200);
    assert.equal(ref.database.twoFactor.length, 1);
    const { symmetricDecrypt } = await import(pathToFileURL(p + 'crypto/index.mjs'));
    const { createOTP } = await import(pathToFileURL(process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/@better-auth/utils/dist/otp.mjs'));
    jar.clear();
    configure({ enabled: false });
    denyFactorWrites = true;
    await call('/sign-in/email', creds);
    const before = JSON.stringify(ref.database.twoFactor[0]);
    const ctx = await auth.$context;
    const totp = await createOTP(await symmetricDecrypt({ key: ctx.secretConfig, data: ref.database.twoFactor[0].secret })).totp();
    assert.equal((await call('/two-factor/verify-totp', { code: totp })).status, 200);
    jar.clear();
    await call('/sign-in/email', creds);
    await call('/two-factor/send-otp');
    assert.equal((await call('/two-factor/verify-otp', { code: otp })).status, 200);
    assert.equal(blockedWrites, 0);
    assert.equal(JSON.stringify(ref.database.twoFactor[0]), before);
    jar.clear();
    await call('/sign-in/email', creds);
    assert.equal((await call('/two-factor/verify-backup-code', { code: enrolled.body.backupCodes[2] })).status, 500);
    assert.equal(blockedWrites, 1);
    console.log('PASS factor UPDATE denied: disabled pending TOTP/OTP require no factor writes; backup consumption still requires mutation');
    console.log('PASS actual lockout: mixed factor threshold, locked OTP retains code, authenticated bypass/no reset, disabled bypass/no mutation, existing expiry despite policy change, equality reset, current threshold, zero duration');
}
finally {
    globalThis.Date = RealDate;
}
