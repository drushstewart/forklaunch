import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
const p = dependencies + '/node_modules/better-auth/dist/';
const { betterAuthReference } = await import(pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const { betterAuth } = await import(pathToFileURL(p + 'index.mjs'));
const { twoFactor } = await import(pathToFileURL(p + 'plugins/two-factor/index.mjs'));
const { symmetricDecrypt } = await import(pathToFileURL(p + 'crypto/index.mjs'));
const secret = 'owned-codec-secret-at-least-32-characters';
const digest = x => createHash('sha256').update(x).digest('base64url');
async function setup(storage, secretOptions = {}) {
    const ref = await betterAuthReference(root);
    ref.database.twoFactor = [];
    let code;
    let delivered = 0;
    let auth;
    function configure(storeOTP, secrets = secretOptions) { auth = betterAuth({ ...ref.config, secret, ...secrets, plugins: [...ref.config.plugins, twoFactor({ otpOptions: { storeOTP, sendOTP: ({ otp }) => { code = otp; delivered++; } } })], rateLimit: { enabled: false }, logger: { disabled: true } }); }
    configure(storage);
    const jar = new Map();
    async function call(route, body = {}) { const r = await auth.handler(new Request('http://localhost:3000/api/auth' + route, { method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json', cookie: [...jar].map(([k, v]) => `${k}=${v}`).join('; ') }, body: JSON.stringify(body) })); for (const c of r.headers.getSetCookie()) {
        const pair = c.split(';')[0], i = pair.indexOf('=');
        jar.set(pair.slice(0, i), pair.slice(i + 1));
    } const t = await r.text(); return { status: r.status, body: t ? JSON.parse(t) : null }; }
    await call('/sign-up/email', { name: 'Codec', firstName: 'Codec', lastName: 'User', email: 'codec@example.test', password: 'owned-factor-password' });
    return { ref, call, configure, context: () => auth.$context, row: () => ref.database.verification.find(r => r.identifier.startsWith('2fa-otp-')), code: () => code, delivered: () => delivered };
}
const { symmetricEncrypt, parseEnvelope } = await import(pathToFileURL(p + 'crypto/index.mjs'));
const { xchacha20poly1305 } = await import(pathToFileURL(dependencies + '/node_modules/@noble/ciphers/chacha.js'));
const observations = [];
const key = createHash('sha256').update(secret).digest();
const nonce = Uint8Array.from({ length: 24 }, (_, i) => i);
const vectors = [];
for (const plaintext of ['0', '012345', '01234567890123456789012345678901', 'é🔑', '']) {
    const ciphertext = Buffer.concat([nonce, xchacha20poly1305(key, nonce).encrypt(new TextEncoder().encode(plaintext))]).toString('hex');
    assert.equal(await symmetricDecrypt({ key: secret, data: ciphertext }), plaintext);
    const random = await symmetricEncrypt({ key: secret, data: plaintext });
    const bytes = Buffer.from(random, 'hex');
    assert.equal(new TextDecoder().decode(xchacha20poly1305(key, bytes.subarray(0, 24)).decrypt(bytes.subarray(24))), plaintext);
    vectors.push({ secret, plaintext, nonce: Buffer.from(nonce).toString('hex'), ciphertext });
}
for (const [from, to] of [['plain', 'encrypted'], ['hashed', 'encrypted'], ['encrypted', 'plain'], ['encrypted', 'hashed']]) {
    const s = await setup(from);
    await s.call('/two-factor/send-otp');
    s.configure(to);
    const result = await s.call('/two-factor/verify-otp', { code: s.code() });
    if (to === 'encrypted') {
        assert.equal(result.status, 500);
        assert.equal(s.row(), undefined);
    }
    else {
        assert.equal(result.body.code, 'INVALID_CODE');
        assert(s.row());
        s.configure(from);
        assert.equal((await s.call('/two-factor/verify-otp', { code: s.code() })).status, 200);
    }
    observations.push({ from, to, status: result.status, consumed: to === 'encrypted' });
}
const s = await setup('encrypted');
await s.call('/two-factor/send-otp');
(await s.context()).secretConfig = 'different-owned-secret-at-least-32-characters';
assert.equal((await s.call('/two-factor/verify-otp', { code: s.code() })).status, 500);
assert.equal(s.row(), undefined);
observations.push({ wrongSecret: '500-consumed' });
const old = { version: 7, value: 'owned-version7-secret-at-least32-characters' };
const newer = { version: 8, value: 'owned-version8-secret-at-least32-characters' };
for (const retain of [true, false]) {
    const s = await setup('encrypted', { secrets: [old] });
    await s.call('/two-factor/send-otp');
    assert(s.row().value.startsWith('$ba$7$'));
    (await s.context()).secretConfig = { keys: new Map((retain ? [newer, old] : [newer]).map(x => [x.version, x.value])), currentVersion: 8 };
    const result = await s.call('/two-factor/verify-otp', { code: s.code() });
    assert.equal(result.status, retain ? 200 : 500);
    assert.equal(s.row(), undefined);
    observations.push({ versionRotation: retain ? 'retained succeeds' : 'retired fails500', consumed: true });
}
const raw = await symmetricEncrypt({ key: secret, data: '123456' });
assert.equal(await symmetricDecrypt({ key: { keys: new Map([[7, old.value]]), currentVersion: 7, legacySecret: secret }, data: raw }), '123456');
await assert.rejects(symmetricDecrypt({ key: { keys: new Map([[7, old.value]]), currentVersion: 7 }, data: raw }));
const versioned = await symmetricEncrypt({ key: { keys: new Map([[7, old.value]]), currentVersion: 7 }, data: '123456' });
assert(versioned.startsWith('$ba$7$'));
assert.equal(await symmetricDecrypt({ key: { keys: new Map([[7, old.value]]), currentVersion: 7 }, data: versioned.replace('$7$', '$7junk$') }), '123456');
assert.equal(parseEnvelope('$ba$-1$abc'), null);
assert.equal(parseEnvelope('$ba$7junk$abc').version, 7);
observations.push({ envelope: '$ba$decimal$ciphertext; parseInt prefix accepted upstream', legacy: 'requires explicit legacySecret in versioned config' });
const fs = await import('node:fs/promises');
assert.deepEqual(JSON.parse(await fs.readFile(path.join(root, 'blueprint-rs/crates/implementations/iam/forklaunch-iam-better-auth/src/two_factor/otp_encryption_vectors.json'), 'utf8')), vectors);
console.log('PASS encrypted OTP actual oracle', JSON.stringify(observations), 'vectors', vectors.length);
// Upstream accepts a switched-to-plain ciphertext proof longer than native's128-byte input cap.
const boundary = await setup('encrypted');
await boundary.call('/two-factor/send-otp');
const longCipher = await symmetricEncrypt({ key: secret, data: '0'.repeat(32) });
assert.equal(longCipher.length, 144);
boundary.row().value = longCipher + ':0';
boundary.configure('plain');
assert.equal((await boundary.call('/two-factor/verify-otp', { code: longCipher })).status, 200);
console.log('PASS upstream144-byte switched-plain ciphertext proof; native128-byte boundary explicitly retained');
