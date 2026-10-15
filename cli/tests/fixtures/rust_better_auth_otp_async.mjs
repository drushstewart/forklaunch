import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const p = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/better-auth/dist/';
const { betterAuthReference } = await import(pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const { betterAuth } = await import(pathToFileURL(p + 'index.mjs'));
const { twoFactor } = await import(pathToFileURL(p + 'plugins/two-factor/index.mjs'));
const secret = 'owned-codec-secret-at-least-32-characters';
const digest = x => createHash('sha256').update(x).digest('base64url');
async function setup(storage) {
    const ref = await betterAuthReference(root);
    ref.database.twoFactor = [];
    let code;
    let delivered = 0;
    let auth;
    function configure(storeOTP) { auth = betterAuth({ ...ref.config, secret, plugins: [...ref.config.plugins, twoFactor({ otpOptions: { storeOTP, sendOTP: ({ otp }) => { code = otp; delivered++; } } })], rateLimit: { enabled: false }, logger: { disabled: true } }); }
    configure(storage);
    const jar = new Map();
    async function call(route, body = {}) { const r = await auth.handler(new Request('http://localhost:3000/api/auth' + route, { method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json', cookie: [...jar].map(([k, v]) => `${k}=${v}`).join('; ') }, body: JSON.stringify(body) })); for (const c of r.headers.getSetCookie()) {
        const pair = c.split(';')[0], i = pair.indexOf('=');
        jar.set(pair.slice(0, i), pair.slice(i + 1));
    } const t = await r.text(); return { status: r.status, body: t ? JSON.parse(t) : null }; }
    await call('/sign-up/email', { name: 'Codec', firstName: 'Codec', lastName: 'User', email: 'codec@example.test', password: 'owned-factor-password' });
    return { ref, call, configure, row: () => ref.database.verification.find(r => r.identifier.startsWith('2fa-otp-')), code: () => code, delivered: () => delivered };
}
const RealDate = Date;
let clock = 1800000000000;
globalThis.Date = class extends RealDate {
    constructor(...args) { super(...(args.length ? args : [clock])); }
    static now() { return clock; }
    static [Symbol.hasInstance](value) { return value instanceof RealDate; }
};
const observations = [];
const originalError = console.error;
console.error = () => { };
for (const action of ['signout', 'expiry', 'resend', 'factor-state-change']) {
    clock = 1800000000000;
    let armed = false;
    let release;
    let reached;
    const entered = new Promise(r => reached = r);
    const gate = new Promise(r => release = r);
    const state = await setup({ hash: async (value) => { if (armed) {
            reached();
            await gate;
        } return digest(value); } });
    await state.call('/two-factor/send-otp');
    const original = state.code();
    const expires = +state.row().expiresAt;
    armed = true;
    const verify = state.call('/two-factor/verify-otp', { code: original });
    await entered;
    assert.equal(state.row(), undefined);
    armed = false;
    let mutation;
    if (action === 'signout')
        mutation = await state.call('/sign-out');
    if (action === 'expiry')
        clock = expires + 1;
    if (action === 'resend')
        mutation = await state.call('/two-factor/send-otp');
    if (action === 'factor-state-change')
        state.ref.database.user[0].twoFactorEnabled = true;
    release();
    const result = await verify;
    observations.push({ action, status: result.status, code: result.body?.code, sessions: state.ref.database.session.length, remainingOTP: !!state.row(), returnedToken: typeof result.body?.token === 'string', mutation: mutation?.status });
}
// Send callback is authorized before await and can publish after signout.
clock = 1800000000000;
let release, reached;
const entered = new Promise(r => reached = r);
const gate = new Promise(r => release = r);
const send = await setup({ hash: async (value) => { reached(); await gate; return digest(value); } });
const response = send.call('/two-factor/send-otp');
await entered;
assert.equal(send.row(), undefined);
await send.call('/sign-out');
release();
const sent = await response;
observations.push({ action: 'send-after-signout', status: sent.status, sessions: send.ref.database.session.length, publishedOTP: !!send.row(), delivered: send.delivered() });
for (const item of observations) {
    assert.equal(item.status, 200);
    if (item.action === 'send-after-signout') {
        assert.equal(item.sessions, 0);
        assert.equal(item.publishedOTP, true);
        assert.equal(item.delivered, 1);
    }
    else {
        assert.equal(item.sessions, 1);
        assert.equal(item.returnedToken, true);
        assert.equal(item.remainingOTP, item.action === 'resend');
    }
}
// A failed asynchronous verifier consumes proof; a failed encoder preserves an existing proof.
for (const operation of ['verify-failure', 'resend-failure']) {
    let fail = false;
    const state = await setup({ hash: async (value) => { if (fail)
            throw new Error('owned sensitive callback failure'); return digest(value); } });
    await state.call('/two-factor/send-otp');
    const saved = structuredClone(state.row());
    const original = state.code();
    fail = true;
    const result = await state.call(operation === 'verify-failure' ? '/two-factor/verify-otp' : '/two-factor/send-otp', { code: original });
    assert.equal(result.status, 500);
    assert.equal(!!state.row(), operation === 'resend-failure');
    if (operation === 'resend-failure')
        assert.equal(JSON.stringify(state.row()) === JSON.stringify(saved), true);
    observations.push({ action: operation, status: result.status, remainingOTP: !!state.row() });
}
// Actual memory adapter allows duplicate rows when concurrent sends await codecs.
{
    let block = false, releaseFirst, enteredFirst;
    const entered = new Promise(r => enteredFirst = r);
    const gate = new Promise(r => releaseFirst = r);
    let first = true;
    const state = await setup({ hash: async (value) => { if (block && first) {
            first = false;
            enteredFirst();
            await gate;
        } return digest(value); } });
    block = true;
    const a = state.call('/two-factor/send-otp');
    await entered;
    const b = await state.call('/two-factor/send-otp');
    assert.equal(b.status, 200);
    const secondId = state.row().id;
    releaseFirst();
    const ar = await a;
    assert.equal(ar.status, 200);
    assert.equal(state.ref.database.verification.filter(row => row.identifier.startsWith('2fa-otp-')).length, 2);
    assert.equal(state.row().id, secondId);
    assert.equal(state.delivered(), 2);
    observations.push({ action: 'simultaneous-send', status: ar.status, duplicateRows: 2, delivered: 2 });
}
// Trusted codecs may deliberately map every proof to empty output; no implicit digit validation is introduced.
{
    const state = await setup({ hash: async () => '' });
    assert.equal((await state.call('/two-factor/send-otp')).status, 200);
    assert.equal((await state.call('/two-factor/verify-otp', { code: '' })).status, 200);
    observations.push({ action: 'empty-custom-hash', status: 200 });
}
{
    const state = await setup({ encrypt: async code => '🔐' + code, decrypt: async code => code.slice('🔐'.length) });
    assert.equal((await state.call('/two-factor/send-otp')).status, 200);
    assert.equal((await state.call('/two-factor/verify-otp', { code: 'é' })).body.code, 'INVALID_CODE');
    assert.equal((await state.call('/two-factor/verify-otp', { code: state.code() })).status, 200);
    observations.push({ action: 'unicode-custom-encryption', status: 200 });
}
globalThis.Date = RealDate;
console.error = originalError;
console.log('PASS actual async OTP codec race/failure boundaries', JSON.stringify(observations));
