// Actual configured organization graph/session/HTTP behavior.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const reference = await betterAuthReference(root);
const RealDate = Date,
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
const owner = reference.client(),
  foreign = reference.client(),
  transient = reference.client();
for (const [client, email, rememberMe] of [
  [owner, 'org-owner@fixture.test', true],
  [foreign, 'org-foreign@fixture.test', true],
  [transient, 'org-transient@fixture.test', false],
]) {
  await atTime(start, () =>
    client.call('/sign-up/email', {
      name: 'Org Person',
      firstName: 'Org',
      lastName: 'Person',
      email,
      password: 'test-only-password',
      rememberMe,
    }),
  );
  await atTime(start, () =>
    client.call('/organization/create', {
      name: email,
      slug: email.split('@')[0],
      metadata: { plan: 'fixture' },
    }),
  );
}
const ownOrg = reference.database.organization[0],
  foreignOrg = reference.database.organization[1];
const ownerId = reference.database.user[0].id;
reference.database.invitation.push({
  id: 'org-fixture-invitation',
  organizationId: ownOrg.id,
  email: 'invited@fixture.test',
  role: 'viewer',
  status: 'pending',
  inviterId: ownerId,
  expiresAt: new RealDate(start + 60000),
  createdAt: new RealDate(start),
});
// A second member makes query limits and user projection observable.
reference.database.member.push({
  id: 'org-fixture-member',
  organizationId: ownOrg.id,
  userId: reference.database.user[2].id,
  role: 'viewer',
  createdAt: new RealDate(start),
});
reference.database.user[0].privateField = 'must-not-appear-in-member-user';
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
const seed = snapshot(),
  seededIds = new Set(
    Object.values(seed)
      .flat()
      .map((row) => row.id),
  );
const inputs = [{ op: 'seed', now: start, ...seed }],
  expected = [];
const attributes = (cookies) =>
  cookies
    .map((cookie) => [cookie.split('=')[0], cookie.slice(cookie.indexOf(';'))])
    .sort();
const headers = (client) => ({
  'content-type': 'application/json',
  origin: 'http://localhost:3000',
  ...(client
    ? {
        cookie: [...client.cookies]
          .map(([key, value]) => `${key}=${value}`)
          .join('; '),
      }
    : {}),
});
async function compare(
  route,
  body,
  client = owner,
  now = start,
  method = body === undefined ? 'GET' : 'POST',
  customHeaders,
) {
  const request = new Request(`http://localhost:3000/api/auth${route}`, {
    method,
    headers: customHeaders ?? headers(client),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const response = await atTime(now, () => reference.auth.handler(request));
  let value = await response.text();
  try {
    value = JSON.parse(value);
  } catch {}
  inputs.push({
    op: 'request',
    now,
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
  inputs.push({ op: 'records', now, withOrganizations: true });
  expected.push(snapshot());
}
for (const route of [
  '/organization/create',
  '/organization/check-slug',
  '/organization/set-active',
]) {
  for (const body of [undefined, null, [], {}, false])
    await compare(route, body, owner, start, 'POST');
}
for (const [route, body] of [
  ['/organization/create', { name: 'Org', slug: 'unauth' }],
  ['/organization/check-slug', { slug: 'unauth' }],
  ['/organization/set-active', {}],
  ['/organization/list', undefined],
  ['/organization/get-full-organization', undefined],
])
  await compare(route, body, null);
await compare('/organization/create', {
  name: '',
  slug: '',
  logo: 2,
  metadata: null,
  keepCurrentActiveOrganization: 1,
  domain: 2,
  subscription: [],
  status: false,
});
await compare('/organization/check-slug', { slug: 123 });
await compare('/organization/check-slug', { slug: ownOrg.slug });
await compare('/organization/check-slug', { slug: ownOrg.slug.toUpperCase() });
await compare('/organization/check-slug', { slug: '' });
for (const query of [
  '',
  `?organizationId=${ownOrg.id}`,
  `?organizationSlug=${ownOrg.slug}&organizationId=absent`,
  '?organizationId=absent',
  `?organizationId=${foreignOrg.id}`,
  '?organizationId=a&organizationId=b&organizationSlug=x&organizationSlug=y&membersLimit=1&membersLimit=2',
])
  await compare('/organization/get-full-organization' + query);
for (const value of [
  '0',
  '1',
  '2',
  '-1',
  'abc',
  '1suffix',
  '0x1',
  '+1',
  '9999999999999999999999999999999',
])
  await compare(
    `/organization/get-full-organization?organizationId=${ownOrg.id}&membersLimit=${encodeURIComponent(value)}`,
  );
for (const body of [
  { organizationId: 123, organizationSlug: null },
  { organizationId: foreignOrg.id },
  { organizationId: 'absent' },
  { organizationSlug: 'absent' },
  {},
  { organizationSlug: ownOrg.slug },
  { organizationId: ownOrg.id, organizationSlug: foreignOrg.slug },
  { organizationId: null, organizationSlug: ownOrg.slug },
  { organizationId: null },
  {},
])
  await compare('/organization/set-active', body);
await compare('/organization/list');
await compare('/organization/get-full-organization');
await compare(
  '/organization/set-active',
  { organizationId: ownOrg.id },
  transient,
);
await compare('/organization/set-active', { organizationId: null }, transient);
for (const body of [
  { name: 'Existing', slug: ownOrg.slug },
  {
    name: 'New',
    slug: 'new-org',
    metadata: { purpose: 'fixture' },
    logo: null,
    domain: 'example.test',
    userId: reference.database.user[1].id,
  },
  { name: 'Kept', slug: 'kept-org', keepCurrentActiveOrganization: true },
  {
    name: 'Null fields',
    slug: 'null-fields',
    domain: null,
    subscription: null,
    status: null,
  },
  { name: 'Uppercase slug', slug: ownOrg.slug.toUpperCase() },
]) {
  await compare('/organization/create', body);
  await compare('/organization/get-full-organization');
}
await compare('/organization/list');
await compare(
  '/organization/create',
  { name: 'No Origin', slug: 'bad-origin' },
  owner,
  start,
  'POST',
  { 'content-type': 'application/json', cookie: headers(owner).cookie },
);
await compare(
  '/organization/set-active',
  { organizationId: ownOrg.id },
  owner,
  start + 3600000,
);
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
    maxBuffer: 32 * 1024 * 1024,
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
function normalize(value, ids = new Map()) {
  if (Array.isArray(value)) return value.map((item) => normalize(item, ids));
  if (value && typeof value === 'object')
    return Object.fromEntries(
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, item]) => {
          if (key === 'membershipKey') {
            assert.equal(
              item,
              createHash('sha256')
                .update(JSON.stringify([value.teamId, value.userId]))
                .digest('base64url'),
            );
            return [key, '[validated team membership key]'];
          }
          return [key, normalize(item, ids)];
        }),
    );
  if (
    typeof value === 'string' &&
    /^[0-9a-f]{8}-[0-9a-f-]{27}$/.test(value) &&
    !seededIds.has(value)
  ) {
    if (!ids.has(value)) ids.set(value, `[generated:${ids.size}]`);
    return ids.get(value);
  }
  return value;
}
const normalizedActual = normalize(actual),
  normalizedExpected = normalize(expected);
const mismatches = [];
for (let i = 0; i < expected.length; i++) {
  try {
    assert.deepEqual(normalizedActual[i], normalizedExpected[i]);
  } catch (error) {
    mismatches.push({
      case: i,
      input: inputs[i + 1].path ?? inputs[i + 1].op,
      diff: error.message,
    });
  }
}
assert.deepEqual(mismatches, []);
console.log(
  `Better Auth organizations: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
