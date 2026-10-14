import assert from 'node:assert/strict';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES is required');
const prefix = pathToFileURL(
  path.join(dependencies, 'node_modules') + '/'
).href;
const { signJWT, resolveSigningKey } = await import(
  prefix + 'better-auth/dist/plugins/jwt/sign.mjs'
);
const { generateKeyPair, exportJWK, decodeProtectedHeader, jwtVerify } =
  await import(prefix + 'jose/dist/webapi/index.js');
const pair = await generateKeyPair('ES256', { extractable: true });
let reads = 0,
  creates = 0;
const row = {
  id: 'stored',
  alg: 'ES256',
  createdAt: new Date(),
  publicKey: JSON.stringify(await exportJWK(pair.publicKey)),
  privateKey: JSON.stringify(await exportJWK(pair.privateKey))
};
let rows = [row];
const options = {
  jwks: { disablePrivateKeyEncryption: true, keyPairConfig: { alg: 'ES256' } },
  adapter: {
    getJwks: async () => {
      reads++;
      return rows;
    },
    createJwk: async () => {
      creates++;
      throw Error('unexpected creation');
    }
  }
};
const ctx = {
  context: { options: { baseURL: 'https://auth.test' }, adapter: {} }
};
const resolved = await resolveSigningKey(ctx, options, {
  signingKeyId: 'stored',
  signingAlgorithm: 'ES256'
});
assert.equal(reads, 1);
assert.equal(creates, 0);
for (const hints of [
  {},
  { signingKeyId: 'missing', signingAlgorithm: 'wrong' }
]) {
  const token = await signJWT(ctx, {
    options,
    payload: { sub: 'user' },
    resolvedKey: resolved,
    ...hints,
    header: { kid: 'header', alg: 'EdDSA', typ: 'custom' }
  });
  assert.deepEqual(decodeProtectedHeader(token), {
    kid: 'stored',
    alg: 'ES256',
    typ: 'custom'
  });
  assert.equal((await jwtVerify(token, pair.publicKey)).payload.sub, 'user');
  assert.equal(reads, 1);
}
row.expiresAt = new Date(0);
assert.ok(await signJWT(ctx, { options, payload: {}, resolvedKey: resolved }));
rows = [];
assert.ok(await signJWT(ctx, { options, payload: {}, resolvedKey: resolved }));
assert.equal(reads, 1);
assert.equal(creates, 0);
await assert.rejects(
  signJWT(ctx, {
    options,
    payload: {},
    resolvedKey: { ...resolved, privateKey: pair.publicKey }
  })
);
await assert.rejects(
  signJWT(ctx, {
    options,
    payload: {},
    resolvedKey: { ...resolved, alg: 'EdDSA' }
  })
);
const renamed = await signJWT(ctx, {
  options,
  payload: {},
  resolvedKey: { ...resolved, kid: 'caller-resolved-id' }
});
assert.equal(decodeProtectedHeader(renamed).kid, 'caller-resolved-id');
let external;
const token = await signJWT(ctx, {
  options: {
    ...options,
    jwt: {
      sign: async (...args) => {
        external = args;
        return 'external';
      }
    }
  },
  payload: {},
  resolvedKey: { alg: 'wrong', kid: 'wrong', privateKey: null },
  signingKeyId: 'requested',
  signingAlgorithm: 'requested-alg',
  header: { kid: 'header' }
});
assert.equal(token, 'external');
assert.deepEqual(external[1], { kid: 'header' });
assert.deepEqual(external[2], {
  signingKeyId: 'requested',
  signingAlgorithm: 'requested-alg'
});
assert.equal(reads, 1);
assert.equal(creates, 0);
console.log(
  'PASS actual resolved-key semantics: one initial resolution, zero reuse reads/creates, hints ignored, key headers authoritative, expiry/removal not rechecked, public/mismatched-key failures, caller kid accepted, external signer wins'
);
