import assert from 'node:assert/strict';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const prefix = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(prefix, 'FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES is required');
const load = relative => import(pathToFileURL(path.join(prefix, 'node_modules', relative)));
const { signJWT, resolveSigningKey } = await load('better-auth/dist/plugins/jwt/sign.mjs');
const { generateKeyPair, exportJWK, decodeProtectedHeader, jwtVerify } = await load('jose/dist/webapi/index.js');
const ctx = { context: { options: { baseURL: 'https://auth.test' }, adapter: new Proxy({}, {
  get() { throw Error('External signing must not access storage'); }
}) } };
const observed = [];
const options = { jwt: { sign: async (...args) => { observed.push(args); return 'opaque'; } } };
const header = { kid: 'caller-key', alg: 'caller-alg', typ: null, extra: [true, 1] };
assert.equal(await resolveSigningKey(ctx, options, { signingKeyId: 'missing', signingAlgorithm: 'unknown' }), null);
assert.equal(await signJWT(ctx, { options, payload: { sub: 'identity' }, header, signingKeyId: 'missing', signingAlgorithm: 'unknown' }), 'opaque');
assert.equal(observed[0][1], header);
assert.deepEqual(observed[0][2], { signingKeyId: 'missing', signingAlgorithm: 'unknown' });
await signJWT(ctx, { options, payload: {} });
assert.equal(observed[1][1], undefined);
assert.deepEqual(observed[1][2], { signingKeyId: undefined, signingAlgorithm: undefined });
await signJWT(ctx, { options, payload: {}, header: {}, signingKeyId: '', signingAlgorithm: '' });
assert.deepEqual(observed[2][1], {});
assert.deepEqual(observed[2][2], { signingKeyId: '', signingAlgorithm: '' });
const failure = Error('private details');
await assert.rejects(signJWT(ctx, { options: { jwt: { sign: async () => { throw failure; } } }, payload: {} }), error => error === failure);
const pair = await generateKeyPair('ES256', { extractable: true });
const row = { id: 'actual-key', alg: 'ES256', createdAt: new Date(), publicKey: JSON.stringify(await exportJWK(pair.publicKey)), privateKey: JSON.stringify(await exportJWK(pair.privateKey)) };
let reads = 0;
const token = await signJWT(ctx, {
  options: { jwks: { disablePrivateKeyEncryption: true, keyPairConfig: { alg: 'ES256' } }, adapter: { getJwks: async () => { reads++; return [row]; } } },
  payload: { sub: 'identity' }, header: { ...header, typ: 'custom' }
});
assert.equal(reads, 1);
assert.deepEqual(decodeProtectedHeader(token), { ...header, typ: 'custom', alg: 'ES256', kid: 'actual-key' });
assert.equal((await jwtVerify(token, pair.publicKey)).payload.sub, 'identity');
console.log('PASS 6 actual external context cases: raw header/hints, absent metadata, empty values, key-resolution bypass, callback failure, local authoritative alg/kid overlay');
