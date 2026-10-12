// Real configured seven-year policy, local-calendar cutoffs, encrypted ORM
// anonymization and native state comparisons.
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
const RealDate = Date;
const previousTimezone = process.env.TZ;
let now = RealDate.parse('2028-02-29T12:34:56.789Z');
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [now]));
  }
  static now() {
    return now;
  }
};
const source = await betterAuthOrm(root, dependencies);
const { persistence, entities, core } = source;
let checks = 0;
let orm;
let schema;
let bridge;
try {
  for (const zone of [
    'UTC',
    'America/Los_Angeles',
    'Australia/Lord_Howe',
    'Pacific/Apia'
  ]) {
    process.env.TZ = zone;
    const child = nativeAuthBridge(root);
    try {
      for (const value of [
        '2028-02-29T12:34:56.789Z',
        '2026-03-10T09:30:00.000Z',
        '2026-11-03T09:30:00.000Z',
        '2026-11-03T10:00:00.000Z',
        '2026-03-10T09:00:00.000Z',
        '2018-12-29T10:00:00.000Z',
        '2026-10-05T15:15:00.000Z',
        '2018-12-29T22:00:00.000Z',
        '2026-01-01T00:00:00.001Z'
      ]) {
        now = RealDate.parse(value);
        const actual = persistence
          .subtractDuration(new Date(), persistence.parseDuration('P7Y'))
          .toISOString();
        const native = await child.native({ op: 'retention-cutoff', now });
        assert.equal(native, actual, `${zone}: ${value}`);
        checks++;
      }
    } finally {
      await child.close();
    }
  }
  process.env.TZ = 'UTC';
  now = RealDate.parse('2028-02-29T12:34:56.789Z');
  persistence.registerEncryptor(
    new persistence.FieldEncryptor(
      'actual-orm-fixture-encryption-key-32-characters'
    )
  );
  schema = `fl_auth_retention_${crypto.randomBytes(8).toString('hex')}`;
  orm = await source.MikroORM.init({
    entities: Object.values(entities),
    clientUrl: process.env.FORKLAUNCH_TEST_POSTGRES_URL,
    schema
  });
  await orm.schema.create();
  const context = (callback) =>
    core.RequestContext.create(orm.em, () =>
      persistence.withEncryptionContext('retention-fixture', callback)
    );
  const reference = await betterAuthReference(root, {
    dependencies,
    orm,
    entities,
    persistence
  });
  const client = reference.client();
  const signup = await context(() =>
    client.call('/sign-up/email', {
      email: 'retention@example.test',
      password: 'retention-fixture-password',
      name: 'Retention Person',
      firstName: 'Retention',
      lastName: 'Person'
    })
  );
  assert.equal(signup.status, 200);
  const userId = signup.body.user.id;
  const cutoff = persistence.subtractDuration(
    new Date(),
    persistence.parseDuration('P7Y')
  );
  const configurations = [
    ['before', cutoff.getTime() - 1, null, true],
    ['exact', cutoff.getTime(), null, true],
    ['after', cutoff.getTime() + 1, null, true],
    ['ancient', RealDate.parse('2000-01-01T00:00:00Z'), null, true],
    [
      'marked',
      RealDate.parse('2000-01-01T00:00:00Z'),
      new Date('2027-01-01T00:00:00Z'),
      true
    ],
    ['empty', RealDate.parse('2000-01-01T00:00:00Z'), null, false],
    ['fresh', now, null, true]
  ];
  const ids = new Map();
  await context(async () => {
    for (const [name, created, marked, secret] of configurations) {
      const id = crypto.randomUUID();
      ids.set(name, id);
      orm.em.create(entities.Account, {
        id,
        user: userId,
        accountId: name,
        providerId: 'fixture',
        createdAt: new Date(created),
        updatedAt: new Date(created),
        retentionAnonymizedAt: marked,
        accessToken: secret ? `retention-access-${name}` : null,
        refreshToken: secret ? `retention-refresh-${name}` : null,
        idToken: secret ? `retention-id-${name}` : null,
        password: secret ? `retention-password-${name}` : null
      });
    }
    await orm.em.flush();
  });
  const authContext = await reference.auth.$context;
  async function accountRows() {
    const rows = await context(() =>
      authContext.adapter.findMany({ model: 'account' })
    );
    const markers = await context(() => orm.em.find(entities.Account, {}));
    for (const row of rows)
      row.retentionAnonymizedAt =
        markers.find((record) => record.id === row.id).retentionAnonymizedAt ??
        null;
    return JSON.parse(JSON.stringify(rows));
  }
  bridge = nativeAuthBridge(root);
  await bridge.native({
    op: 'replace-seed',
    now,
    accounts: await accountRows()
  });
  const { RetentionService } = source.load(
    'framework/core/src/services/retentionService.ts'
  );
  const service = new RetentionService(orm, {
    info() {},
    warn() {},
    error() {}
  });
  const normalize = (value) => ({
    ...JSON.parse(JSON.stringify(value)),
    durationMs: 0
  });
  async function enforce(options = {}) {
    const actual = normalize(
      await service.enforce({ ...options, tenantIds: ['retention-fixture'] })
    );
    const native = normalize(
      await bridge.native({ op: 'retention', now, ...options })
    );
    assert.deepEqual(native, actual, JSON.stringify(options));
    const nativeRows = (await bridge.native({ op: 'records', now })).accounts;
    const sort = (rows) => rows.sort((a, b) => a.id.localeCompare(b.id));
    assert.deepEqual(sort(nativeRows), sort(await accountRows()));
    checks++;
    return native;
  }
  const dry = await enforce({ dryRun: true, batchSize: 1 });
  assert.equal(dry.anonymized, 3);
  await enforce({ entities: ['Jwks'] });
  const processed = await enforce({ batchSize: 1 });
  assert.equal(processed.anonymized, 3);
  assert.equal(processed.deleted, 0);
  assert.equal(processed.errors, 0);
  await enforce();
  await enforce({ dryRun: true });
  const rows = await accountRows();
  for (const name of ['before', 'ancient', 'empty']) {
    const row = rows.find((row) => row.id === ids.get(name));
    for (const field of ['accessToken', 'refreshToken', 'idToken', 'password'])
      assert.equal(row[field], null);
    assert.equal(row.retentionAnonymizedAt, new Date().toISOString());
  }
  for (const name of ['exact', 'after', 'fresh', 'marked'])
    assert.equal(
      rows.find((row) => row.id === ids.get(name)).accessToken,
      `retention-access-${name}`
    );
  console.log(
    `Configured Better Auth retention: ${checks} actual timezone/calendar/ORM/native policy and state comparisons passed`
  );
} finally {
  try {
    if (bridge) await bridge.close();
  } finally {
    try {
      if (orm)
        await orm.em
          .getConnection()
          .execute(`DROP SCHEMA IF EXISTS "${schema}" CASCADE`);
    } finally {
      if (orm) await orm.close(true);
      globalThis.Date = RealDate;
      if (previousTimezone === undefined) delete process.env.TZ;
      else process.env.TZ = previousTimezone;
    }
  }
}
