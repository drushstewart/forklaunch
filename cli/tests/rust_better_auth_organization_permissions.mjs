// Actual stored dynamic authorization, scoped organization mutations and errors.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const reference = await betterAuthReference(root),
  RealDate = Date,
  start = 1800000000000;
async function atTime(now, work) {
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [now]));
    }
    static [Symbol.hasInstance](value) {
      return value instanceof RealDate;
    }
    static now() {
      return now;
    }
  };
  try {
    return await work();
  } finally {
    globalThis.Date = RealDate;
  }
}
const clients = [];
for (let i = 0; i < 7; i++) {
  const client = reference.client();
  clients.push(client);
  await atTime(start, () =>
    client.call('/sign-up/email', {
      name: 'Permission Person',
      firstName: 'Permission',
      lastName: 'Person',
      email: `org-permission-${i}@fixture.test`,
      password: 'test-only-password',
    }),
  );
  if (i < 6)
    await atTime(start, () =>
      client.call('/organization/create', {
        name: `Org ${i}`,
        slug: `permission-${i}`,
        metadata: { source: 'fixture' },
      }),
    );
}
const organizations = reference.database.organization;
reference.database.member.find(
  (row) => row.organizationId === organizations[1].id,
).role = 'viewer,writer';
for (const [index, role, permission] of [
  [0, 'owner', '{"organization":["update","delete"]}'],
  [1, 'writer', '{"organization":["update"]}'],
  [3, 'owner', '{'],
  [4, 'owner', '{"organization":[1]}'],
  [5, 'owner', '{"organization":1e400}'],
])
  reference.database.organizationRole.push({
    id: `dynamic-role-${index}`,
    organizationId: organizations[index].id,
    role,
    permission,
    createdAt: new RealDate(start),
    updatedAt: new RealDate(start),
  });
for (let i = 0; i < 3; i++)
  reference.database.invitation.push({
    id: `permission-invite-${i}`,
    organizationId: organizations[i].id,
    email: `invited-${i}@fixture.test`,
    role: 'viewer',
    status: 'pending',
    inviterId: reference.database.user[i].id,
    createdAt: new RealDate(start),
    expiresAt: new RealDate(start + 60000),
  });
const secondary = {
  ...reference.database.session[0],
  id: 'secondary-owner-session',
  token: 'secondary-owner-token',
};
reference.database.session.push(secondary);
const fields = {
  users: 'user',
  accounts: 'account',
  sessions: 'session',
  organizations: 'organization',
  members: 'member',
  teams: 'team',
  teamMembers: 'teamMember',
  invitations: 'invitation',
  organizationRoles: 'organizationRole',
};
const snapshot = () =>
  JSON.parse(
    JSON.stringify(
      Object.fromEntries(
        Object.entries(fields).map(([key, model]) => [
          key,
          reference.database[model],
        ]),
      ),
    ),
  );
const inputs = [{ op: 'seed', now: start, ...snapshot() }],
  expected = [];
const attributes = (cookies) =>
  cookies
    .map((cookie) => [cookie.split('=')[0], cookie.slice(cookie.indexOf(';'))])
    .sort();
async function compare(
  route,
  body,
  client = clients[0],
  method = body === undefined ? 'GET' : 'POST',
) {
  const request = new Request(`http://localhost:3000/api/auth${route}`, {
    method,
    headers: {
      'content-type': 'application/json',
      origin: 'http://localhost:3000',
      ...(client
        ? {
            cookie: [...client.cookies]
              .map(([key, value]) => `${key}=${value}`)
              .join('; '),
          }
        : {}),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const response = await atTime(start, () => reference.auth.handler(request));
  let value = await response.text();
  try {
    value = JSON.parse(value);
  } catch {}
  inputs.push({
    op: 'request',
    now: start,
    path: route,
    method,
    headers: Object.fromEntries(request.headers),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  expected.push({
    status: response.status,
    body: value,
    cookies: attributes(response.headers.getSetCookie()),
  });
  inputs.push({ op: 'records', now: start, withOrganizations: true });
  expected.push(snapshot());
}
for (const route of [
  '/organization/update',
  '/organization/delete',
  '/organization/has-permission',
])
  for (const body of [undefined, null, [], {}, false])
    await compare(route, body, clients[0], 'POST');
await compare(
  '/organization/update',
  { data: {}, organizationId: organizations[0].id },
  null,
);
await compare(
  '/organization/delete',
  { organizationId: organizations[0].id },
  null,
);
await compare(
  '/organization/has-permission',
  { permissions: { platform: ['platform:read'] } },
  null,
);
await compare('/organization/get-organization', undefined, null);
await compare('/organization/update', {
  data: {
    domain: 2,
    subscription: [],
    status: false,
    name: '',
    slug: '',
    logo: 3,
    metadata: null,
  },
  organizationId: 3,
});
await compare('/organization/update', { data: null, organizationId: null });
for (const body of [
  {},
  { organizationId: 2 },
  { permissions: {} },
  { permission: {} },
  { permissions: {}, permission: {} },
  { permissions: { organization: 'update' } },
  { permissions: { organization: [1] } },
  { permissions: { organization: [] } },
  { permissions: { platform: ['platform:read'], organization: ['update'] } },
  { permission: { organization: ['update'] } },
  { permissions: { organization: ['update'] }, permission: 'invalid' },
])
  await compare('/organization/has-permission', body);
for (let i = 0; i < 7; i++) {
  for (const permissions of [
    { platform: ['platform:read'] },
    { platform: ['platform:write'] },
    { organization: ['update'] },
    { organization: ['delete'] },
    { platform: ['platform:read'], organization: ['update'] },
    { organization: ['update', 'delete'] },
    { unknown: ['read'] },
  ])
    await compare('/organization/has-permission', { permissions }, clients[i]);
}
for (const route of [
  '/organization/update',
  '/organization/delete',
  '/organization/has-permission',
])
  await compare(route, {
    organizationId: organizations[2].id,
    ...(route.endsWith('update')
      ? { data: { name: 'Forbidden' } }
      : route.endsWith('has-permission')
        ? { permissions: { platform: ['platform:read'] } }
        : {}),
  });
for (const query of [
  '',
  `?organizationId=${organizations[0].id}`,
  `?organizationSlug=${organizations[0].slug}&organizationId=absent`,
  '?organizationId=absent',
  `?organizationId=${organizations[2].id}`,
  '?organizationId=a&organizationId=b&organizationSlug=x&organizationSlug=y&membersLimit=1&membersLimit=2',
])
  await compare('/organization/get-organization' + query);
await compare('/organization/has-permission', {
  permissions: { platform: ['platform:read'] },
});
await compare('/organization/update', { data: {} });
await compare('/organization/set-active', {
  organizationId: organizations[0].id,
});
await compare('/organization/update', { data: {} });
await compare('/organization/update', {
  data: { slug: organizations[1].slug },
});
await compare('/organization/update', {
  data: {
    name: 'Changed',
    slug: 'changed-org',
    logo: null,
    domain: null,
    subscription: 'paid',
    status: 'paused',
    metadata: { source: 'updated' },
    id: 'injected',
    createdAt: 'injected',
  },
});
await compare('/organization/get-organization');
await compare(
  '/organization/update',
  { data: { name: 'Static role denied' } },
  clients[2],
);
await compare(
  '/organization/update',
  { data: { name: 'Writer allowed' } },
  clients[1],
);
await compare(
  '/organization/delete',
  { organizationId: organizations[1].id },
  clients[1],
);
await compare('/organization/delete', { organizationId: '' });
await compare('/organization/delete', { organizationId: 'absent' });
const removedId = organizations[0].id;
await compare('/organization/delete', { organizationId: removedId });
await compare(`/organization/get-organization?organizationId=${removedId}`);
await compare('/organization/has-permission', {
  organizationId: removedId,
  permissions: { platform: ['platform:read'] },
});
await compare('/organization/list');
const run = spawnSync(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    'blueprint-rs/Cargo.toml',
    '-p',
    'forklaunch-iam-better-auth',
    '--features',
    'http',
    '--example',
    'http_parity',
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    input: inputs.map((input) => JSON.stringify(input)).join('\n') + '\n',
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  },
);
assert.equal(run.status, 0, run.stderr);
const raw = run.stdout
  .trim()
  .split('\n')
  .map((line) => JSON.parse(line))
  .slice(1);
assert.equal(raw.length, expected.length);
const actual = raw.map((value, i) =>
  inputs[i + 1].op === 'request'
    ? {
        status: value.status,
        body: value.body,
        cookies: attributes(value.headers['set-cookie'] ?? []),
      }
    : value,
);
const mismatches = [];
for (let i = 0; i < expected.length; i++) {
  try {
    assert.deepEqual(actual[i], expected[i]);
  } catch (error) {
    mismatches.push({
      case: i,
      path: inputs[i + 1].path ?? inputs[i + 1].op,
      diff: error.message,
    });
  }
}
assert.deepEqual(mismatches, []);
console.log(
  `Better Auth organization permissions: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
