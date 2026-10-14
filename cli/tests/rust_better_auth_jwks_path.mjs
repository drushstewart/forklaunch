// Actual configured plugin fixed-path subset. Native tests additionally reject
// unsafe/dynamic paths and collisions before Axum construction.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
let comparisons = 0;
for (const jwksPath of ['/jwks', '/.well-known/jwks.json', '/keys/public', '/']) {
  for (const remote of [false, true]) {
    const { auth } = await betterAuthReference(root, { jwt: { jwks: {
      jwksPath,
      ...(remote ? { remoteUrl: 'https://keys.example.test/public', keyPairConfig: { alg: 'EdDSA' } } : {}),
    } } });
    for (const method of ['GET', 'HEAD', 'POST']) {
      const response = await auth.handler(new Request(`http://localhost:3000/api/auth${jwksPath}`, { method }));
      assert.equal(response.status, method === 'GET' && !remote ? 200 : 404);
      if (method === 'HEAD') assert.equal(await response.text(), '');
      comparisons++;
    }
    if (jwksPath === '/') {
      const bare = await auth.handler(new Request('http://localhost:3000/api/auth'));
      assert.equal(bare.status, 404);
      const descendant = await auth.handler(new Request('http://localhost:3000/api/auth/unknown'));
      assert.equal(descendant.status, 404);
      comparisons += 2;
    }
    if (jwksPath !== '/jwks') {
      const response = await auth.handler(new Request('http://localhost:3000/api/auth/jwks'));
      assert.equal(response.status, 404);
      comparisons++;
    }
    const response = await auth.handler(new Request('http://localhost:3000/api/auth/open-api/generate-schema'));
    assert.equal(response.status, 200);
    const schema = await response.json();
    assert.ok(schema.paths[jwksPath]);
    if (jwksPath !== '/jwks') assert.equal(schema.paths['/jwks'], undefined);
    comparisons++;
  }
}
for (const jwksPath of ['', 'keys', '/a..b']) {
  await assert.rejects(() => betterAuthReference(root, { jwt: { jwks: { jwksPath } } }));
  comparisons++;
}
console.log(`Better Auth fixed JWKS path: ${comparisons} route/schema/config cases PASS.`);
