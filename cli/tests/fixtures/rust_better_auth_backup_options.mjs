import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const p = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/better-auth/dist/';
const { generateBackupCodes } = await import(pathToFileURL(p + 'plugins/two-factor/backup-codes/index.mjs'));
const { symmetricDecrypt } = await import(pathToFileURL(p + 'crypto/index.mjs'));
const key = 'owned-backup-generator-audit-secret-at-least32';
for (const [amount, length] of [[undefined, undefined], [0, 0], [2, 1], [3, 5], [2, 6], [2, 127]]) {
    const options = { amount, length, storeBackupCodes: 'encrypted' };
    const generated = await generateBackupCodes(key, options);
    assert.equal(generated.backupCodes.length, amount ?? 10);
    for (const code of generated.backupCodes) {
        assert.equal(code.length, (length ?? 10) + 1);
        assert.equal(code[Math.min(5, length ?? 10)], '-');
        assert.match(code, /^[a-zA-Z0-9]*-[a-zA-Z0-9]*$/);
    }
    assert.deepEqual(JSON.parse(await symmetricDecrypt({ key, data: generated.encryptedBackupCodes })), generated.backupCodes);
}
await assert.rejects(generateBackupCodes(key, { amount: 1, length: 0 }), /positive integer/);
let calls = 0;
const codes = ['duplicate', 'duplicate', '', 'é🔑'];
const opts = { amount: 1, length: 1, storeBackupCodes: 'encrypted', customBackupCodesGenerate: () => { calls++; return codes; } };
const generated = await generateBackupCodes(key, opts);
assert.deepEqual(generated.backupCodes, codes);
assert.equal(calls, 1);
await assert.rejects(generateBackupCodes(key, { customBackupCodesGenerate: () => { throw Error('owned generation failure'); } }), /owned generation failure/);
const { betterAuthReference } = await import(pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const { betterAuth } = await import(pathToFileURL(p + 'index.mjs'));
const { twoFactor } = await import(pathToFileURL(p + 'plugins/two-factor/index.mjs'));
const ref = await betterAuthReference(root);
ref.database.twoFactor = [];
let fail = false;
calls = 0;
const auth = betterAuth({ ...ref.config, plugins: [...ref.config.plugins, twoFactor({ skipVerificationOnEnable: true, backupCodeOptions: { customBackupCodesGenerate: () => { calls++; if (fail)
                    throw Error('owned generation failure'); return codes; } } })], logger: { disabled: true }, rateLimit: { enabled: false } });
const jar = new Map();
async function call(route, body = {}) { const r = await auth.handler(new Request('http://localhost:3000/api/auth' + route, { method: 'POST', headers: { origin: 'http://localhost:3000', 'content-type': 'application/json', cookie: [...jar].map(([k, v]) => k + '=' + v).join('; ') }, body: JSON.stringify(body) })); const cookies = r.headers.getSetCookie(); for (const c of cookies) {
    const pair = c.split(';')[0], i = pair.indexOf('=');
    if (c.includes('Max-Age=0'))
        jar.delete(pair.slice(0, i));
    else
        jar.set(pair.slice(0, i), pair.slice(i + 1));
} const text = await r.text(); return { status: r.status, body: text ? JSON.parse(text) : null, cookies }; }
const password = 'owned-factor-password';
assert.equal((await call('/two-factor/enable', { password })).status, 401);
assert.equal(calls, 0);
await call('/sign-up/email', { name: 'Backup', firstName: 'Backup', lastName: 'Test', email: 'backup@example.test', password });
assert.equal((await call('/two-factor/enable', { password: 'wrong' })).status, 400);
assert.equal(calls, 0);
const enabled = await call('/two-factor/enable', { password });
assert.equal(enabled.status, 200);
assert.deepEqual(enabled.body.backupCodes, codes);
assert.equal(calls, 1);
assert.equal((await call('/two-factor/enable', { password })).body.code, 'TOTP_ALREADY_ENABLED');
assert.equal(calls, 1);
assert.equal((await call('/two-factor/verify-backup-code', { code: 'duplicate' })).status, 200);
assert.equal((await call('/two-factor/verify-backup-code', { code: 'duplicate' })).status, 401);
assert.equal((await call('/two-factor/verify-backup-code', { code: '' })).status, 200);
assert.equal((await call('/two-factor/verify-backup-code', { code: 'é🔑' })).status, 200);
const before = JSON.stringify(ref.database.twoFactor);
const sessions = JSON.stringify(ref.database.session);
fail = true;
const failed = await call('/two-factor/generate-backup-codes', { password });
assert.equal(failed.status, 500);
assert.equal(failed.body, null);
assert.equal(failed.cookies.length, 0);
assert.equal(JSON.stringify(ref.database.twoFactor), before);
assert.equal(JSON.stringify(ref.database.session), sessions);
assert.equal(calls, 2);
console.log('PASS actual backup generation: six amount/length profiles plus zero-length rejection; fixed index5 dash; custom override/duplicates/empty/Unicode; call ordering and generator failure preserves factor/session');
