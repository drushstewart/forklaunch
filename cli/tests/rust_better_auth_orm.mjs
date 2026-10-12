// Real configured MikroORM/Better Auth/PostgreSQL integration. The fixture owns
// only its isolated schema, encryption tenant, credentials and request context.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthOrm } from './fixtures/rust_better_auth_orm.mjs';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
import { nativeAuthBridge } from './fixtures/rust_better_auth_bridge.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const dependencies = process.env.FORKLAUNCH_TEST_ORM_DEPENDENCIES;
const databaseUrl = process.env.FORKLAUNCH_TEST_POSTGRES_URL;
assert.ok(
  dependencies,
  'Set FORKLAUNCH_TEST_ORM_DEPENDENCIES to pinned ORM fixture packages'
);
assert.ok(
  databaseUrl,
  'Set FORKLAUNCH_TEST_POSTGRES_URL to the isolated test database'
);
const source = await betterAuthOrm(root, dependencies);
const { persistence, entities, core } = source;
const require = createRequire(path.join(dependencies, 'package.json'));
const jose = await import(pathToFileURL(require.resolve('jose')));
persistence.registerEncryptor(
  new persistence.FieldEncryptor(
    'actual-orm-fixture-encryption-key-32-characters'
  )
);
const schema = `fl_auth_orm_${crypto.randomBytes(8).toString('hex')}`;
const orm = await source.MikroORM.init({
  entities: Object.values(entities),
  clientUrl: databaseUrl,
  schema
});
const events = [];
const unsubscribe = persistence.onComplianceAccess((event) =>
  events.push(event)
);
const context = (callback) =>
  core.RequestContext.create(orm.em, () =>
    persistence.withEncryptionContext('orm-fixture', callback)
  );
let checks = 0;
try {
  await orm.schema.create();
  const options = { dependencies, orm, entities, persistence };
  let reference = await betterAuthReference(root, options);
  const client = reference.client();
  async function call(route, body, status = 200) {
    const result = await context(() => client.call(route, body));
    assert.equal(
      result.status,
      status,
      `${route}: ${JSON.stringify(result.body)}`
    );
    checks++;
    return result;
  }
  const credentials = {
    email: 'orm@example.test',
    password: 'orm-fixture-password',
    name: 'ORM Person',
    firstName: 'ORM',
    lastName: 'Person'
  };
  const signup = await call('/sign-up/email', credentials);
  const userId = signup.body.user.id;
  assert.equal(signup.body.user.email, credentials.email);
  const session = await call('/get-session');
  assert.equal(session.body.user.id, userId);
  const jwks = await call('/jwks');
  const jwt = await jose.jwtVerify(
    session.headers.get('set-auth-jwt'),
    jose.createLocalJWKSet(jwks.body),
    { issuer: 'http://localhost:3000', audience: 'http://localhost:3000' }
  );
  assert.equal(jwt.payload.sub, userId);
  assert.equal(jwt.payload.email, credentials.email);
  checks++;
  await call(
    '/sign-in/email',
    { email: credentials.email, password: 'wrong-password' },
    401
  );
  await call('/sign-in/email', credentials);
  const authContext = await reference.auth.$context;
  const account = await context(() =>
    authContext.adapter.findOne({
      model: 'account',
      where: [{ field: 'userId', value: userId }]
    })
  );
  assert.equal(typeof account.password, 'string');
  assert.match(account.password, /^[a-f0-9]+:[a-f0-9]+$/);
  assert.ok(
    events.some(
      (event) =>
        event.field === 'Account.password' && event.tenantId === 'orm-fixture'
    )
  );
  assert.ok(
    events.some(
      (event) =>
        event.field === 'Jwks.privateKey' && event.tenantId === 'orm-fixture'
    )
  );
  checks++;
  await assert.rejects(
    core.RequestContext.create(orm.em, () =>
      authContext.adapter.findOne({
        model: 'account',
        where: [{ field: 'id', value: account.id }]
      })
    ),
    /tenant|context/i
  );
  await assert.rejects(
    core.RequestContext.create(orm.em, () =>
      persistence.withEncryptionContext('other-tenant', () =>
        authContext.adapter.findOne({
          model: 'account',
          where: [{ field: 'id', value: account.id }]
        })
      )
    ),
    /decrypt|corrupt|key/i
  );
  checks += 2;
  const raw = await orm.em
    .getConnection()
    .execute(`select password from "${schema}".account`);
  assert.match(raw[0].password, /^v4:/);
  assert.ok(!raw[0].password.includes(account.password));
  const keys = await orm.em
    .getConnection()
    .execute(`select private_key from "${schema}".jwks`);
  assert.match(keys[0].private_key, /^v4:/);
  assert.ok(!keys[0].private_key.includes('"d":'));
  checks++;
  await context(async () => {
    const entity = await orm.em.findOne(entities.Account, { id: account.id });
    assert.ok(persistence.isCompliantField(entity.password));
    assert.equal(JSON.stringify(entity.password), '{}');
    assert.ok(entity.createdAt instanceof Date);
  });
  checks++;
  const linked = await context(() =>
    authContext.adapter.transaction(async (transaction) => {
      const row = await transaction.create({
        model: 'account',
        data: {
          userId,
          accountId: 'provider-subject',
          providerId: 'fixture-provider',
          accessToken: 'private-provider-access',
          refreshToken: 'private-provider-refresh',
          idToken: 'private-provider-id',
          createdAt: new Date(),
          updatedAt: new Date()
        }
      });
      assert.equal(row.accessToken, 'private-provider-access');
      assert.equal(row.refreshToken, 'private-provider-refresh');
      assert.ok(row.createdAt instanceof Date);
      return row;
    })
  );
  checks++;
  await context(() =>
    authContext.adapter.transaction(async (transaction) => {
      const row = await transaction.update({
        model: 'account',
        where: [{ field: 'id', value: linked.id }],
        update: { accessToken: 'private-provider-updated' }
      });
      assert.equal(row.accessToken, 'private-provider-updated');
    })
  );
  checks++;
  await assert.rejects(
    context(() =>
      authContext.adapter.transaction(async (transaction) => {
        await transaction.update({
          model: 'account',
          where: [{ field: 'id', value: linked.id }],
          update: { accessToken: 'must-roll-back' }
        });
        throw new Error('owned-transaction-rollback');
      })
    ),
    /owned-transaction-rollback/
  );
  const reread = await context(() =>
    authContext.adapter.findOne({
      model: 'account',
      where: [{ field: 'id', value: linked.id }]
    })
  );
  assert.equal(reread.accessToken, 'private-provider-updated');
  checks++;
  const list = await call('/list-accounts');
  assert.ok(!JSON.stringify(list.body).includes('private-provider'));
  assert.ok(!JSON.stringify(list.body).includes(account.password));
  const stored = await orm.em
    .getConnection()
    .execute(
      `select access_token,refresh_token,id_token from "${schema}".account where id=?`,
      [linked.id]
    );
  for (const value of Object.values(stored[0])) assert.match(value, /^v4:/);
  checks++;
  // Import actual ORM-adapter records into the native protocol store. This
  // exercises real encrypted-column reads, dates, hashes and session cookies.
  const bridge = nativeAuthBridge(root);
  try {
    const seed = { op: 'replace-seed', now: Date.now() };
    for (const [model, field] of [
      ['user', 'users'],
      ['account', 'accounts'],
      ['session', 'sessions']
    ]) {
      seed[field] = await context(() =>
        authContext.adapter.findMany({ model })
      );
    }
    await bridge.native(seed);
    const cookie = [...client.cookies]
      .map(([name, value]) => `${name}=${value}`)
      .join('; ');
    const nativeRequest = (route, body) =>
      bridge.native({
        op: 'request',
        now: Date.now(),
        path: route,
        method: body ? 'POST' : 'GET',
        headers: {
          cookie,
          origin: 'http://localhost:3000',
          'content-type': 'application/json'
        },
        body: body ? JSON.stringify(body) : ''
      });
    const actualSession = await call('/get-session');
    const nativeSession = await nativeRequest('/get-session');
    assert.equal(nativeSession.status, 200);
    assert.deepEqual(nativeSession.body, actualSession.body);
    checks++;
    const nativeJwks = await nativeRequest('/jwks');
    const nativeJwt = await jose.jwtVerify(
      nativeSession.headers['set-auth-jwt'][0],
      jose.createLocalJWKSet(nativeJwks.body),
      { issuer: 'http://localhost:3000', audience: 'http://localhost:3000' }
    );
    assert.equal(nativeJwt.payload.sub, userId);
    checks++;
    const nativeAccounts = await nativeRequest('/list-accounts');
    assert.equal(nativeAccounts.status, 200);
    assert.deepEqual(nativeAccounts.body, list.body);
    checks++;
    const nativeSignIn = await nativeRequest('/sign-in/email', credentials);
    assert.equal(nativeSignIn.status, 200);
    assert.equal(nativeSignIn.body.user.id, userId);
    assert.equal(nativeSignIn.body.user.email, credentials.email);
    const nativeRejected = await nativeRequest('/sign-in/email', {
      email: credentials.email,
      password: 'wrong-password'
    });
    assert.equal(nativeRejected.status, 401);
    checks += 2;
  } finally {
    await bridge.close();
  }
  // Recreate the configured auth instance and its request client: no identity
  // map or cached key may hide failures reading encrypted database values.
  reference = await betterAuthReference(root, options);
  const restarted = reference.client();
  const signed = await context(() =>
    restarted.call('/sign-in/email', credentials)
  );
  assert.equal(signed.status, 200);
  const resumed = await context(() => restarted.call('/get-session'));
  assert.equal(resumed.status, 200);
  assert.equal(resumed.body.user.id, userId);
  assert.ok(resumed.headers.get('set-auth-jwt'));
  checks += 2;
  console.log(
    `Configured Better Auth real ORM: ${checks} PostgreSQL/auth/encryption/audit/transaction/restart checks passed`
  );
} finally {
  unsubscribe();
  try {
    await orm.em
      .getConnection()
      .execute(`DROP SCHEMA IF EXISTS "${schema}" CASCADE`);
  } finally {
    await orm.close(true);
  }
}
