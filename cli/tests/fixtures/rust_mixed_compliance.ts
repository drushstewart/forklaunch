import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createComplianceClient } from './compliance.js';
import { iamSdkClient, legacyApiSdkClient } from './clientSdk.js';
import { createForkLaunchSdk } from '../../sdks/typescript/src/index.js';
import type { RegistryOptions } from '@forklaunch/universal-sdk';

const userId = '12345678-1234-4234-9234-123456789abc';
const requests: {
  service: string;
  action: string;
  userId: string;
  token: string | undefined;
}[] = [];
const server = createServer((request, response) => {
  const [service, , action, id] = request
    .url!.split('/')
    .filter(Boolean)
    .map(decodeURIComponent);
  requests.push({
    service,
    action,
    userId: id,
    token: request.headers.authorization
  });
  assert.ok(['iam', 'native'].includes(service));
  assert.equal(request.method, action === 'erase' ? 'DELETE' : 'GET');
  assert.equal(request.headers.authorization, 'Bearer fixture-system-token');
  response.setHeader('content-type', 'application/json');
  if (service === 'native' && action === 'erase' && id === 'conflict') {
    response.writeHead(409).end(JSON.stringify({ conflict: 'retained' }));
  } else {
    response.end(
      JSON.stringify(
        action === 'erase'
          ? {
              recordsDeleted: service === 'native' ? 2 : 1,
              entitiesAffected: [service]
            }
          : { userId: id, entities: { [service]: ['record'] } }
      )
    );
  }
});
await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
const address = server.address();
assert.ok(address && typeof address !== 'string');
const base = `http://127.0.0.1:${address.port}`;
try {
  const raw: Extract<
    Extract<RegistryOptions, { raw: unknown }>['raw'],
    { openapi: string }
  > = JSON.parse(
    readFileSync('../../.forklaunch/openapi/native-api/openapi.json', 'utf8')
  );
  if ('paths' in raw && raw.paths) {
    raw.paths['/compliance/erase/{userId}']!.delete!.operationId =
      'compliance.eraseUserData';
    raw.paths['/compliance/export/{userId}']!.get!.operationId =
      'compliance.exportUserData';
  }
  const iam = await iamSdkClient({
    host: base + '/iam',
    registryOptions: { raw }
  });
  const legacy = await legacyApiSdkClient({
    host: base + '/legacy',
    registryOptions: { raw }
  });
  const shared = createForkLaunchSdk({
    serviceBaseUrls: { 'native-api': base + '/native' }
  });
  const nativeApi = { compliance: shared.nativeApi.compliance };
  const legacyCalls: { action: string; userId: string; token: string }[] = [];
  type Request = {
    params: { userId: string };
    headers: { authorization: string };
  };
  const legacyApi = Object.assign(legacy, {
    compliance: {
      eraseUserData: async ({ params, headers }: Request) => {
        legacyCalls.push({
          action: 'erase',
          userId: params.userId,
          token: headers.authorization
        });
        return { code: 202 as const, response: { source: 'legacy' as const } };
      },
      exportUserData: async ({ params, headers }: Request) => {
        legacyCalls.push({
          action: 'export',
          userId: params.userId,
          token: headers.authorization
        });
        return { code: 200 as const, response: { entries: ['legacy'] } };
      }
    }
  });
  if (false) {
    createComplianceClient({
      token: '',
      iam,
      nativeApi,
      // @ts-expect-error A service created before IAM needs declared handlers or an explicit adapter.
      legacyApi: legacy
    });
  }
  assert.throws(
    () =>
      Reflect.apply(createComplianceClient, undefined, [
        { token: '', iam, nativeApi: {}, legacyApi }
      ]),
    /native-api/
  );
  assert.throws(
    () =>
      Reflect.apply(createComplianceClient, undefined, [
        { token: '', iam, nativeApi, legacyApi: {} }
      ]),
    /legacy-api/
  );
  assert.equal(requests.length, 0);
  assert.equal(legacyCalls.length, 0);
  const client = createComplianceClient({
    token: 'fixture-system-token',
    iam,
    nativeApi,
    legacyApi
  });
  const erased = await client.erase(userId);
  assert.deepEqual(Object.keys(erased).sort(), [
    'iam',
    'legacyApi',
    'nativeApi'
  ]);
  assert.equal(erased.nativeApi.code, 200);
  if (erased.nativeApi.code === 200) {
    const count: number = erased.nativeApi.response.recordsDeleted;
    assert.equal(count, 2);
    // @ts-expect-error Native SDK result models must retain their actual field types.
    const wrong: string = erased.nativeApi.response.recordsDeleted;
    void wrong;
  }
  if (erased.iam.code === 200) {
    const count: number = erased.iam.response.recordsDeleted;
    assert.equal(count, 1);
  }
  const accepted: 202 = erased.legacyApi.code;
  const source: 'legacy' = erased.legacyApi.response.source;
  assert.equal(accepted, 202);
  assert.equal(source, 'legacy');
  const exported = await client.export(userId);
  if (exported.nativeApi.code === 200) {
    const records: string[] = exported.nativeApi.response.entities.native;
    assert.deepEqual(records, ['record']);
  } else assert.fail('Native export failed');
  assert.deepEqual(exported.legacyApi.response.entries, ['legacy']);
  const conflict = await client.erase('conflict');
  assert.equal(conflict.nativeApi.code, 409);
  if (conflict.nativeApi.code === 409) {
    const reason: string = conflict.nativeApi.response.conflict;
    assert.equal(reason, 'retained');
  }
  assert.equal(requests.length, 6);
  assert.equal(legacyCalls.length, 3);
  assert.ok(
    requests.every((request) => request.token === 'Bearer fixture-system-token')
  );
  assert.ok(
    legacyCalls.every(
      (request) => request.token === 'Bearer fixture-system-token'
    )
  );
  assert.equal(
    requests.filter((request) => request.userId === userId).length,
    4
  );
  console.log(
    'PASS mixed compliance fan-out through actual legacy/generated SDKs: typed responses, bearer propagation, business errors and required-adapter validation'
  );
} finally {
  server.closeAllConnections();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
