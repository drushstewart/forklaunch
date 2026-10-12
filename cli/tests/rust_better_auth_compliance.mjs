// Compare the actual configured compliance walker on real encrypted ORM rows
// with the native tenant-bound service. Telemetry and schema lifetime are owned.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthOrm } from './fixtures/rust_better_auth_orm.mjs';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
import { nativeAuthBridge } from './fixtures/rust_better_auth_bridge.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const dependencies = process.env.FORKLAUNCH_TEST_ORM_DEPENDENCIES;
assert.ok(dependencies);
assert.ok(process.env.FORKLAUNCH_TEST_POSTGRES_URL);
const source = await betterAuthOrm(root, dependencies);
const { persistence, entities, core } = source;
persistence.registerEncryptor(
  new persistence.FieldEncryptor(
    'actual-orm-fixture-encryption-key-32-characters'
  )
);
const schema = `fl_auth_compliance_${crypto.randomBytes(8).toString('hex')}`;
const orm = await source.MikroORM.init({
  entities: Object.values(entities),
  clientUrl: process.env.FORKLAUNCH_TEST_POSTGRES_URL,
  schema
});
const context = (callback) =>
  core.RequestContext.create(orm.em, () =>
    persistence.withEncryptionContext('compliance-fixture', callback)
  );
const bridge = nativeAuthBridge(root);
const clean = (value) => JSON.parse(JSON.stringify(value));
let checks = 0;
try {
  await orm.schema.create();
  const reference = await betterAuthReference(root, {
    dependencies,
    orm,
    entities,
    persistence
  });
  const client = reference.client();
  const credentials = {
    email: 'compliance@example.test',
    password: 'compliance-fixture-password',
    name: 'Compliance Person',
    firstName: 'Compliance',
    lastName: 'Person'
  };
  const signup = await context(() =>
    client.call('/sign-up/email', credentials)
  );
  assert.equal(signup.status, 200);
  const userId = signup.body.user.id;
  const otherClient = reference.client();
  const other = await context(() =>
    otherClient.call('/sign-up/email', {
      ...credentials,
      email: 'other@example.test'
    })
  );
  assert.equal(other.status, 200);
  assert.equal((await context(() => client.call('/get-session'))).status, 200);
  const authContext = await reference.auth.$context;
  const linked = await context(() =>
    authContext.adapter.create({
      model: 'account',
      data: {
        userId,
        accountId: 'subject',
        providerId: 'fixture',
        accessToken: 'compliance-access-secret',
        refreshToken: 'compliance-refresh-secret',
        idToken: 'compliance-id-secret',
        createdAt: new Date(),
        updatedAt: new Date()
      }
    })
  );
  const key = await context(() =>
    authContext.adapter.create({
      model: 'jwks',
      forceAllowId: true,
      data: {
        id: userId,
        publicKey: '{}',
        privateKey: 'compliance-private-key',
        alg: 'EdDSA',
        crv: 'Ed25519',
        createdAt: new Date('2000-01-01T00:00:00Z'),
        updatedAt: new Date('2000-01-01T00:00:00Z')
      }
    })
  );
  assert.equal(key.id, userId);
  const seed = { op: 'replace-seed', now: Date.now() };
  for (const [model, field] of [
    ['user', 'users'],
    ['account', 'accounts'],
    ['session', 'sessions']
  ]) {
    seed[field] = await context(() => authContext.adapter.findMany({ model }));
  }
  await bridge.native(seed);
  await bridge.native({
    op: 'seed-keys',
    now: Date.now(),
    keys: await context(() => authContext.adapter.findMany({ model: 'jwks' }))
  });
  const { ComplianceDataService } = source.load(
    'framework/core/src/services/complianceDataService.ts'
  );
  const service = new ComplianceDataService(
    orm,
    { info() {}, warn() {}, error() {} },
    { User: 'id' }
  );
  function failedService(model) {
    const metadata = [...orm.getMetadata().getAll().values()].find(
      (metadata) => metadata.className === model
    );
    const failedOrm = {
      getMetadata: () => orm.getMetadata(),
      em: {
        fork() {
          const manager = orm.em.fork();
          return new Proxy(manager, {
            get(target, key) {
              if (key === 'find')
                return (...args) => {
                  if (
                    args[0] === metadata.class ||
                    args[0] === metadata.className
                  )
                    throw new Error('owned compliance read failure');
                  return Reflect.apply(target.find, target, args);
                };
              const value = Reflect.get(target, key, target);
              return typeof value === 'function' ? value.bind(target) : value;
            }
          });
        }
      }
    };
    return new ComplianceDataService(
      failedOrm,
      { info() {}, warn() {}, error() {} },
      { User: 'id' }
    );
  }
  const scope = { tenantIds: ['compliance-fixture'] };
  for (const user of [userId, other.body.user.id, crypto.randomUUID()]) {
    const actual = clean(await service.export(user, scope));
    const native = await bridge.native({
      op: 'compliance-export',
      now: Date.now(),
      userId: user
    });
    assert.deepEqual(native, actual);
    assert.ok(!JSON.stringify(native).includes('compliance-access-secret'));
    assert.ok(!JSON.stringify(native).includes('compliance-private-key'));
    checks++;
  }
  for (const model of ['Account', 'Jwks']) {
    const actual = clean(await failedService(model).export(userId, scope));
    const native = await bridge.native({
      op: 'compliance-export',
      now: Date.now(),
      userId,
      failComplianceModel: model,
      tenantLabel: 'compliance-fixture'
    });
    assert.deepEqual(native, actual);
    assert.equal(native.failures.length, 1);
    assert.equal(native.failures[0].entityName, model);
    checks++;
  }
  const exported = clean(await service.export(userId, scope));
  assert.equal(exported.entities.Account.length, 2);
  assert.equal(exported.entities.Jwks.length, 1);
  assert.deepEqual(
    exported.entities.Account.find((row) => row.id === linked.id).accessToken,
    {}
  );
  assert.equal(
    exported.entities.Account.find((row) => row.id === linked.id).password,
    null
  );
  const before = {
    users: await context(() => orm.em.count(entities.User, {})),
    sessions: await context(() => orm.em.count(entities.Session, {})),
    accounts: await context(() => orm.em.count(entities.Account, {})),
    keys: await context(() => orm.em.count(entities.Jwks, {}))
  };
  const actualErase = clean(await service.erase(userId, scope));
  const nativeErase = await bridge.native({
    op: 'compliance-erase',
    now: Date.now(),
    userId
  });
  assert.deepEqual(nativeErase, actualErase);
  assert.equal(nativeErase.recordsDeleted, 3);
  checks++;
  assert.equal(
    await context(() => orm.em.count(entities.User, {})),
    before.users
  );
  assert.equal(
    await context(() => orm.em.count(entities.Session, {})),
    before.sessions
  );
  assert.equal(
    await context(() => orm.em.count(entities.Account, {})),
    before.accounts - 2
  );
  assert.equal(
    await context(() => orm.em.count(entities.Jwks, {})),
    before.keys - 1
  );
  const nativeRows = await bridge.native({ op: 'records', now: Date.now() });
  assert.equal(nativeRows.users.length, before.users);
  assert.equal(nativeRows.sessions.length, before.sessions);
  assert.equal(nativeRows.accounts.length, before.accounts - 2);
  checks++;
  assert.deepEqual(
    await bridge.native({ op: 'compliance-export', now: Date.now(), userId }),
    clean(await service.export(userId, scope))
  );
  assert.deepEqual(
    await bridge.native({ op: 'compliance-erase', now: Date.now(), userId }),
    clean(await service.erase(userId, scope))
  );
  checks += 2;
  const surviving = await context(() => client.call('/get-session'));
  assert.equal(surviving.status, 200);
  assert.equal(surviving.body.user.id, userId);
  const denied = await context(() =>
    client.call('/sign-in/email', credentials)
  );
  assert.equal(denied.status, 401);
  checks++;
  for (const model of ['Account', 'Jwks']) {
    await context(() =>
      authContext.adapter.create({
        model: 'account',
        data: {
          userId,
          accountId: 'failure-subject',
          providerId: 'fixture',
          password: 'failure-private-password',
          createdAt: new Date(),
          updatedAt: new Date()
        }
      })
    );
    await context(() =>
      authContext.adapter.create({
        model: 'jwks',
        forceAllowId: true,
        data: {
          id: userId,
          privateKey: 'failure-private-key',
          publicKey: '{}',
          createdAt: new Date('2000-01-01T00:00:00Z'),
          updatedAt: new Date('2000-01-01T00:00:00Z')
        }
      })
    );
    const child = nativeAuthBridge(root);
    try {
      const seed = { op: 'replace-seed', now: Date.now() };
      for (const [family, field] of [
        ['user', 'users'],
        ['account', 'accounts'],
        ['session', 'sessions']
      ])
        seed[field] = await context(() =>
          authContext.adapter.findMany({ model: family })
        );
      await child.native(seed);
      await child.native({
        op: 'seed-keys',
        now: Date.now(),
        keys: await context(() =>
          authContext.adapter.findMany({ model: 'jwks' })
        )
      });
      const actual = clean(await failedService(model).erase(userId, scope));
      const native = await child.native({
        op: 'compliance-erase',
        now: Date.now(),
        userId,
        failComplianceModel: model,
        tenantLabel: 'compliance-fixture'
      });
      assert.deepEqual(native, actual);
      assert.equal(native.recordsDeleted, 1);
      assert.equal(native.failures.length, 1);
      assert.deepEqual(
        await child.native({
          op: 'compliance-export',
          now: Date.now(),
          userId
        }),
        clean(await service.export(userId, scope))
      );
      checks += 2;
    } finally {
      await child.close();
    }
    await context(() =>
      authContext.adapter.deleteMany({
        model: 'account',
        where: [{ field: 'userId', value: userId }]
      })
    );
    await context(() =>
      authContext.adapter.delete({
        model: 'jwks',
        where: [{ field: 'id', value: userId }]
      })
    );
  }
  console.log(
    `Configured Better Auth compliance: ${checks} real ORM/native export/erase/classification comparisons passed`
  );
} finally {
  try {
    await bridge.close();
  } finally {
    try {
      await orm.em
        .getConnection()
        .execute(`DROP SCHEMA IF EXISTS "${schema}" CASCADE`);
    } finally {
      await orm.close(true);
    }
  }
}
