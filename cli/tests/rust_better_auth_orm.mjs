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
const {
  AUTH_ENCRYPTION_CONTEXT,
  AUTH_ENCRYPTION_POLICY,
  createAuthEncryptionOrm,
  withAuthEncryptionPolicy
} = source.load(
  'blueprint/iam-better-auth/domain/utils/authEncryptionPolicy.util.ts'
);
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
  const options = {
    dependencies,
    orm: createAuthEncryptionOrm(orm),
    entities,
    persistence
  };
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
  assert.equal(jwt.payload.sessionId, session.body.session.id);
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
        event.field === 'Account.password' &&
        event.tenantId === AUTH_ENCRYPTION_CONTEXT
    )
  );
  assert.ok(
    events.some(
      (event) =>
        event.field === 'Jwks.privateKey' &&
        event.tenantId === AUTH_ENCRYPTION_CONTEXT
    )
  );
  checks++;
  // Auth identities and signing keys belong to the IAM service. The adapter
  // establishes that context even without a tenant or inside another org scope.
  for (const tenant of [undefined, 'other-tenant']) {
    await core.RequestContext.create(orm.em, async () => {
      const read = async () => {
        const before = persistence.getCurrentTenantId();
        const found = await authContext.adapter.findOne({
          model: 'account',
          where: [{ field: 'id', value: account.id }]
        });
        assert.equal(found.password, account.password);
        assert.equal(persistence.getCurrentTenantId(), before);
      };
      if (tenant) await persistence.withEncryptionContext(tenant, read);
      else await read();
    });
  }
  checks += 2;
  // Direct ORM access still requires the correct encryption owner.
  await assert.rejects(
    core.RequestContext.create(
      orm.em,
      async () =>
        (await orm.em.findOne(entities.Account, { id: account.id })).password
          .deanon
    ),
    /tenant|context/i
  );
  await assert.rejects(
    context(
      async () =>
        (await orm.em.findOne(entities.Account, { id: account.id })).password
          .deanon
    ),
    /decrypt|corrupt|key/i
  );
  checks += 2;
  // Markers are inspected before hydration, including after a record has been
  // cached. Never reinterpret legacy ciphertext as service-owned ciphertext.
  for (const marker of [null, 'legacy-org-policy']) {
    await orm.em
      .getConnection()
      .execute(
        `update "${schema}".account set encryption_policy=? where id=?`,
        [marker, account.id]
      );
    await assert.rejects(
      context(() =>
        authContext.adapter.findOne({
          model: 'account',
          where: [{ field: 'id', value: account.id }]
        })
      ),
      /Legacy auth encryption policy/
    );
  }
  await orm.em
    .getConnection()
    .execute(`update "${schema}".account set encryption_policy=? where id=?`, [
      AUTH_ENCRYPTION_POLICY,
      account.id
    ]);
  checks += 2;
  const raw = await orm.em
    .getConnection()
    .execute(`select password,encryption_policy from "${schema}".account`);
  assert.equal(raw[0].encryption_policy, AUTH_ENCRYPTION_POLICY);
  assert.match(raw[0].password, /^v4:/);
  assert.ok(!raw[0].password.includes(account.password));
  const keys = await orm.em
    .getConnection()
    .execute(`select private_key,encryption_policy from "${schema}".jwks`);
  assert.equal(keys[0].encryption_policy, AUTH_ENCRYPTION_POLICY);
  assert.match(keys[0].private_key, /^v4:/);
  assert.ok(!keys[0].private_key.includes('"d":'));
  checks++;
  await orm.em
    .getConnection()
    .execute(`update "${schema}".jwks set encryption_policy=null`);
  await assert.rejects(
    context(() => authContext.adapter.findMany({ model: 'jwks' })),
    /Legacy auth encryption policy/
  );
  await orm.em
    .getConnection()
    .execute(`update "${schema}".jwks set encryption_policy=?`, [
      AUTH_ENCRYPTION_POLICY
    ]);
  checks++;
  await context(() =>
    withAuthEncryptionPolicy(async () => {
      const entity = await orm.em.findOne(entities.Account, { id: account.id });
      assert.ok(persistence.isCompliantField(entity.password));
      assert.equal(JSON.stringify(entity.password), '{}');
      assert.ok(entity.createdAt instanceof Date);
    })
  );
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
  const organization = await call('/organization/create', {
    name: 'ORM Organization',
    slug: `orm-${crypto.randomUUID()}`,
    domain: 'orm.example.test'
  });
  // Populate beyond the configured default page, through the real adapter.
  // This bypasses member-creation policy solely to exercise listing storage.
  await context(async () => {
    for (let index = 0; index < 120; index++) {
      const now = new Date();
      const memberUser = await authContext.adapter.create({model:'user',data:{name:`Page ${index}`,firstName:'Page',lastName:String(index),email:`page-${index}@orm-fixture.test`,emailVerified:false,createdAt:now,updatedAt:now}});
      await authContext.adapter.create({model:'member',data:{userId:memberUser.id,organizationId:organization.body.id,role:'member',createdAt:now}});
    }
  });
  const membershipQueries=['','?limit=2','?offset=2&limit=2','?offset=120&limit=2','?offset=121&limit=2','?limit=0'];
  const membershipPages=[];
  for (const query of membershipQueries) {
    const result=await call(`/organization/list-members${query}`);
    assert.equal(result.body.total,121);
    membershipPages.push(result.body);
    checks++;
  }
  assert.equal(membershipPages[0].members.length,100);
  assert.equal(membershipPages[3].members.length,1);
  assert.equal(membershipPages[4].members.length,0);
  assert.equal(membershipPages[5].members.length,100);
  const scopedSession = await call('/get-session');
  const scope = {
    userId,
    organizationId: organization.body.id,
    sessionId: scopedSession.body.session.id
  };
  const scopeCases = [
    [scope, 'owner'],
    [{ ...scope, sessionId: scope.sessionId.toUpperCase() }, 'owner'],
    [{ ...scope, organizationId: scope.organizationId.toUpperCase() }, null],
    [{ ...scope, userId: scope.userId.toUpperCase() }, null],
    [{ ...scope, sessionId: crypto.randomUUID() }, null]
  ];
  const scopedResults = [];
  for (const [selected, role] of scopeCases) {
    const result = await context(async () => ({
      role: await reference.surfacing.surfaceRole(selected),
      permissions: Array.from(
        await reference.surfacing.surfacePermissions(selected)
      )
    }));
    assert.equal(result.role, role);
    assert.deepEqual(
      result.permissions,
      role ? ['platform:read', 'platform:write'] : []
    );
    scopedResults.push(result);
    checks++;
  }
  // Import actual ORM-adapter records into the native protocol store. This
  // exercises real encrypted-column reads, dates, hashes and session cookies.
  const bridge = nativeAuthBridge(root);
  try {
    const seed = { op: 'replace-seed', now: Date.now() };
    for (const [model, field] of [
      ['user', 'users'],
      ['account', 'accounts'],
      ['session', 'sessions'],
      ['organization', 'organizations'],
      ['member', 'members'],
      ['organizationRole', 'organizationRoles']
    ]) {
      seed[field] = await context(() =>
        authContext.adapter.findMany({ model, limit:1000 })
      );
    }
    await bridge.native(seed);
    for (const [index, [selected]] of scopeCases.entries()) {
      assert.deepEqual(
        await bridge.native({
          op: 'surface',
          now: Date.now(),
          scope: selected
        }),
        scopedResults[index]
      );
      checks++;
    }
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
    for (const [index,query] of membershipQueries.entries()) {
      const result=await nativeRequest(`/organization/list-members${query}`);
      assert.equal(result.status,200);
      assert.deepEqual(result.body,membershipPages[index]);
      checks++;
    }
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
    assert.equal(nativeJwt.payload.sessionId, actualSession.body.session.id);
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
