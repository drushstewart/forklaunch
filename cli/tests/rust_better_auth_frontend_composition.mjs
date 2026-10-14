// Real source SDK/factory against an owned HTTP wire fixture; no SDK/AJV stubs.
import assert from 'node:assert/strict';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  actualUniversalSdk,
  frontendFactory
} from './fixtures/rust_better_auth_frontend_client.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const { UniversalSdk, universalSdk } = await actualUniversalSdk();
const { default: Ajv } = await import(
  pathToFileURL(
    path.join(
      process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES,
      'node_modules/ajv/dist/ajv.js'
    )
  )
);
let payload = { email: true };
const requests = [];
const server = http.createServer((req, res) => {
  requests.push(req.url);
  res.setHeader('content-type', 'application/json');
  if (req.url === '/discovery/auth-methods') res.end(JSON.stringify(payload));
  else if (req.url.endsWith('/get-session')) res.end('null');
  else {
    res.statusCode = 404;
    res.end('{"error":"not found"}');
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const host = `http://127.0.0.1:${server.address().port}`;
const schema = (type) => ({
  openapi: '3.1.0',
  info: { title: 'fixture', version: '1' },
  paths: {
    '/discovery/auth-methods': {
      get: {
        operationId: 'probe',
        responses: {
          200: {
            content: {
              'application/json': {
                schema: { $ref: '#/components/schemas/Response' }
              }
            }
          }
        }
      }
    }
  },
  components: {
    schemas: {
      Response: {
        type: 'object',
        properties: { email: { type } },
        required: ['email']
      }
    }
  }
});
try {
  const factory = await frontendFactory(
    process.env.FORKLAUNCH_TEST_GENERATED_CLIENT_SDK ??
      path.join(root, 'blueprint/client-sdk/clientSdk.ts')
  );
  for (const authBasePath of [undefined, '/identity/login']) {
    const sdk = await factory({
      host,
      authBasePath,
      registryOptions: { raw: schema('boolean') }
    });
    assert.deepEqual((await sdk.core.probe()).response, payload);
    assert.equal((await sdk.betterAuth.getSession()).data, null);
    assert.equal(requests.at(-1), `${authBasePath ?? '/api/auth'}/get-session`);
  }
  const prefixed = await factory({
    host: host + '/custom/auth',
    registryOptions: { raw: schema('boolean') }
  });
  await assert.rejects(prefixed.core.probe(), /Response 404 not found/);
  assert.equal(requests.at(-1), '/custom/auth/discovery/auth-methods');
  const separated = await factory({
    host: host + '/custom/auth',
    coreHost: host,
    registryOptions: { raw: schema('boolean') }
  });
  assert.equal((await separated.core.probe()).code, 200);
  assert.equal((await separated.betterAuth.getSession()).data, null);
  assert.equal(requests.at(-1), '/custom/auth/get-session');

  const nested = schema('boolean');
  nested.components.schemas.Response = {
    $id: 'https://schemas.fixture.test/response',
    type: 'object',
    properties: { email: { $ref: '#/$defs/Flag' } },
    required: ['email'],
    $defs: { Flag: { type: 'boolean' } }
  };
  const nestedClient = await universalSdk({
    host,
    registryOptions: { raw: nested }
  });
  assert.equal((await nestedClient.probe()).code, 200);
  payload = { email: { invalid: true } };
  await assert.rejects(nestedClient.probe(), /Response does not match/);

  const other = structuredClone(nested);
  other.components.schemas.Response.$defs.Flag = { type: 'object' };
  const otherClient = await universalSdk({
    host,
    registryOptions: { raw: other }
  });
  assert.equal((await otherClient.probe()).code, 200);
  await assert.rejects(nestedClient.probe(), /Response does not match/);
  const versioned = await universalSdk({
    host,
    registryOptions: { raw: { latest: other, v1: nested } }
  });
  assert.equal((await versioned.probe()).code, 200);
  await assert.rejects(versioned.probe['1'](), /Response does not match/);

  const missing = schema('boolean');
  missing.paths['/discovery/auth-methods'].get.responses[200].content[
    'application/json'
  ].schema.$ref = '#/components/schemas/Absent';
  const unresolved = await universalSdk({
    host,
    registryOptions: { raw: missing }
  });
  const before = requests.length;
  await assert.rejects(unresolved.probe(), /can't resolve reference/);
  assert.equal(requests.length, before + 1, 'No schema network fetch');

  const ajv = new Ajv({ strict: false });
  ajv.addKeyword({
    keyword: 'fixtureAllowed',
    type: 'object',
    validate: (_schema, value) => value.email === true
  });
  const custom = schema('boolean');
  custom.components.schemas.Response.fixtureAllowed = true;
  const direct = new UniversalSdk(host, ajv, { raw: custom }, undefined);
  payload = { email: false };
  await assert.rejects(
    direct.executeSdkCall('probe'),
    /Response does not match/
  );
  payload = { email: true };
  assert.equal((await direct.executeSdkCall('probe')).code, 200);
  for (const allowed of [true, false]) {
    const booleanComponent = schema('boolean');
    booleanComponent.components.schemas.Response = allowed;
    const client = await universalSdk({
      host,
      registryOptions: { raw: booleanComponent }
    });
    if (allowed) assert.equal((await client.probe()).code, 200);
    else await assert.rejects(client.probe(), /Response does not match/);
  }
  const shared = new Ajv({ strict: false });
  const firstShared = new UniversalSdk(
    host,
    shared,
    { raw: nested },
    undefined
  );
  assert.equal((await firstShared.executeSdkCall('probe')).code, 200);
  const conflictingShared = new UniversalSdk(
    host,
    shared,
    { raw: other },
    undefined
  );
  await assert.rejects(
    conflictingShared.executeSdkCall('probe'),
    /already exists/
  );
  const remote = schema('boolean');
  remote.paths['/discovery/auth-methods'].get.responses[200].content[
    'application/json'
  ].schema.$ref = `${host}/untrusted-schema`;
  const remoteClient = await universalSdk({
    host,
    registryOptions: { raw: remote }
  });
  const beforeRemote = requests.length;
  await assert.rejects(remoteClient.probe(), /can't resolve reference/);
  assert.equal(requests.length, beforeRemote + 1);
  console.log(
    `PASS real frontend composition (${requests.length} owned HTTP requests): auth paths, explicit coreHost/default prefix preservation, component/nested refs, schema IDs, cross-client/version isolation, unresolved refs without fetch, supplied AJV custom keyword`
  );
} finally {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}
