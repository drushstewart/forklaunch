import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const p = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/better-auth/dist/';
const { betterAuthReference } = await import(pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const { betterAuth } = await import(pathToFileURL(p + 'index.mjs'));
const { twoFactor } = await import(pathToFileURL(p + 'plugins/two-factor/index.mjs'));
const { symmetricDecrypt } = await import(pathToFileURL(p + 'crypto/index.mjs'));
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
const observations = [];
for (const mode of [undefined, 'plain', 'hashed', 'encrypted']) {
    const s = await setup(mode);
    assert.equal((await s.call('/two-factor/send-otp')).status, 200);
    const encoded = s.row().value.split(':')[0];
    if (mode === 'hashed')
        assert.equal(encoded, digest(s.code()));
    else if (mode === 'encrypted')
        assert.equal(await symmetricDecrypt({ key: secret, data: encoded }), s.code());
    else
        assert.equal(encoded, s.code());
    assert.equal((await s.call('/two-factor/verify-otp', { code: 'wrong' })).body.code, 'INVALID_CODE');
    assert.equal(s.row().value, encoded + ':1');
    assert.equal((await s.call('/two-factor/verify-otp', { code: s.code() })).status, 200);
    assert.equal(s.row(), undefined);
    observations.push({ mode: mode ?? 'default', stored: mode === 'hashed' ? 'SHA256-base64url' : mode === 'encrypted' ? 'secret-encrypted' : 'plain', wrongAttemptPreservesEncoding: true });
}
for (const [from, to] of [['plain', 'hashed'], ['hashed', 'plain']]) {
    const s = await setup(from);
    await s.call('/two-factor/send-otp');
    const original = s.row().value;
    s.configure(to);
    assert.equal((await s.call('/two-factor/verify-otp', { code: s.code() })).body.code, 'INVALID_CODE');
    assert.equal(s.row().value, original.slice(0, -1) + '1');
    s.configure(from);
    assert.equal((await s.call('/two-factor/verify-otp', { code: s.code() })).status, 200);
    observations.push({ from, to, currentVerifyPolicy: true, restoreWorks: true });
}
for (const mode of ['plain', 'hashed']) {
    const s = await setup(mode);
    await s.call('/two-factor/send-otp');
    for (const value of ['', 'é🔑', 'not-digits']) {
        const result = await s.call('/two-factor/verify-otp', { code: value });
        assert.equal(result.status, 401);
        assert.equal(result.body.code, 'INVALID_CODE');
    }
    const before = s.row().value;
    const malformed = await s.call('/two-factor/verify-otp', { code: 123 });
    assert.equal(s.row().value, before);
    observations.push({ mode, invalidStrings: 'INVALID_CODE', malformed });
    assert.equal((await s.call('/two-factor/verify-otp', { code: s.code() })).status, 200);
}
let calls = [];
let fail = false;
const custom = { hash: async (code) => { calls.push(code); if (fail)
        throw Error('owned codec failure'); return digest(code); } };
const s = await setup(custom);
assert.equal((await s.call('/two-factor/send-otp')).status, 200);
assert.deepEqual(calls, [s.code()]);
assert.equal(s.delivered(), 1);
fail = true;
assert.equal((await s.call('/two-factor/verify-otp', { code: s.code() })).status, 500);
assert.equal(s.row(), undefined);
assert.equal((await s.call('/two-factor/verify-otp', { code: s.code() })).body.code, 'OTP_HAS_EXPIRED');
const t = await setup(custom);
assert.equal((await t.call('/two-factor/send-otp')).status, 500);
assert.equal(t.delivered(), 0);
assert.equal(t.row(), undefined);
observations.push({ customHash: 'awaited', verifyFailure: '500-consumes-before-callback', sendFailure: '500-no-row-no-delivery' });
let release;
const gate = new Promise(r => release = r);
let entered = false;
const pending = await setup({ hash: async (code) => { entered = true; await gate; return digest(code); } });
let settled = false;
const sending = pending.call('/two-factor/send-otp').then(r => { settled = true; return r; });
await new Promise(r => setTimeout(r, 30));
assert(entered);
assert(!settled);
assert.equal(pending.row(), undefined);
assert.equal(pending.delivered(), 0);
release();
assert.equal((await sending).status, 200);
observations.push({ pendingHash: 'send waits, no row or delivery until resolved' });
let encrypt = 0, decrypt = 0;
const enc = await setup({ encrypt: async (code) => { encrypt++; return 'X' + code; }, decrypt: async (code) => { decrypt++; return code.slice(1); } });
await enc.call('/two-factor/send-otp');
assert.equal(enc.row().value, 'X' + enc.code() + ':0');
assert.equal((await enc.call('/two-factor/verify-otp', { code: enc.code() })).status, 200);
assert.equal(encrypt, 1);
assert.equal(decrypt, 1);
observations.push({ customEncryption: 'one encrypt at send, one decrypt at verify' });
console.log('PASS OTP storage actual oracle', JSON.stringify(observations));
