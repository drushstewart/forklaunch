// Actual team mutation authorization, invitation cleanup and schema ordering.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
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
      name: `Member ${i}`,
      firstName: 'Member',
      lastName: `${i}`,
      email: `member-${i}@fixture.test`,
      password: 'test-only-password',
    }),
  );
}
await atTime(start, () =>
  clients[0].call('/organization/create', {
    name: 'Members A',
    slug: 'members-a',
  }),
);
await atTime(start, () =>
  clients[4].call('/organization/create', {
    name: 'Members B',
    slug: 'members-b',
  }),
);
const organizations = reference.database.organization,
  users = reference.database.user;
const team = reference.database.team[0],
  org = organizations[0];
for (const [index, role] of [
  [1, 'owner'],
  [2, 'member'],
  [3, 'viewer'],
  [6, 'viewer'],
]) {
  await atTime(start, () =>
    reference.auth.api.addMember({
      body: {
        userId: users[index].id,
        role,
        organizationId: org.id,
        teamId: team.id,
      },
    }),
  );
  const session = reference.database.session.find(
    (row) => row.userId === users[index].id,
  );
  session.activeOrganizationId = org.id;
  session.activeTeamId = team.id;
}
users[0].privateField = 'must-not-appear-in-joined-member';
reference.database.organizationRole.push({
  id: 'member-manager-grant',
  organizationId: org.id,
  role: 'viewer',
  permission: '{"member":["update","delete"]}',
  createdAt: new RealDate(start),
});
reference.database.organizationRole.push({
  id: 'member-malformed-grant',
  organizationId: organizations[1].id,
  role: 'owner',
  permission: '{',
  createdAt: new RealDate(start),
});
const members = Object.fromEntries(
  reference.database.member.map((member) => [member.userId, { ...member }]),
);
reference.database.organizationRole.find(
  (row) => row.organizationId === org.id && row.role === 'viewer',
).permission = JSON.stringify({ team: ['create', 'update', 'delete'] });
for (const id of ['mutation-team-a', 'mutation-team-b'])
  reference.database.team.push({
    id,
    organizationId: org.id,
    name: id,
    memberCount: 0,
    createdAt: new RealDate(start),
  });
reference.database.teamMember.push({
  id: 'mutation-membership',
  teamId: 'mutation-team-a',
  userId: users[2].id,
  createdAt: new RealDate(start),
});
reference.database.team.find(
  (row) => row.id === 'mutation-team-a',
).memberCount = 1;
reference.database.session.find(
  (row) => row.userId === users[2].id,
).activeTeamId = 'mutation-team-a';
reference.database.organization.push({
  id: 'mutation-last-org',
  name: 'Last',
  slug: 'mutation-last-org',
  createdAt: new RealDate(start),
});
reference.database.member.push({
  id: 'mutation-last-member',
  organizationId: 'mutation-last-org',
  userId: users[3].id,
  role: 'viewer',
  createdAt: new RealDate(start),
});
reference.database.organizationRole.push({
  id: 'mutation-last-grant',
  organizationId: 'mutation-last-org',
  role: 'viewer',
  permission: JSON.stringify({ team: ['delete'] }),
});
reference.database.team.push({
  id: 'mutation-last-team',
  organizationId: 'mutation-last-org',
  name: 'Last',
  memberCount: 0,
  createdAt: new RealDate(start),
});
for (const [id, status, expiresAt, teamId] of [
  [
    'live-multi',
    'pending',
    start + 50000,
    'mutation-team-a,mutation-team-b,mutation-team-a',
  ],
  ['live-single', 'pending', start + 50000, 'mutation-team-a'],
  ['accepted', 'accepted', start + 50000, 'mutation-team-a'],
  ['expired', 'pending', start - 1, 'mutation-team-a'],
  ['at-expiry', 'pending', start, 'mutation-team-a'],
]) {
  reference.database.invitation.push({
    id: 'mutation-invite-' + id,
    organizationId: org.id,
    email: 'invite-' + id + '@fixture.test',
    role: 'member',
    status,
    expiresAt: new RealDate(expiresAt),
    teamId,
    inviterId: users[0].id,
    createdAt: new RealDate(start),
  });
}
for (let i = 0; i < 101; i++)
  reference.database.invitation.push({
    id: 'mutation-pending-' + i,
    organizationId: org.id,
    email: `pending-${i}@fixture.test`,
    role: 'member',
    status: 'pending',
    expiresAt: new RealDate(start + 50000),
    teamId: 'mutation-team-a',
    inviterId: users[0].id,
    createdAt: new RealDate(start),
  });
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
  '/organization/create-team',
  '/organization/update-team',
  '/organization/remove-team',
]) {
  for (const body of [undefined, null, [], false, {}])
    await compare(route, body, clients[0], 'POST');
}
await compare('/organization/create-team', { name: 'No auth' }, null);
await compare(
  '/organization/create-team',
  { name: 'No auth', organizationId: org.id },
  null,
);
await compare(
  '/organization/update-team',
  { teamId: 'mutation-team-a', data: {} },
  null,
);
await compare('/organization/remove-team', { teamId: 'mutation-team-a' }, null);
await compare(
  '/organization/remove-team',
  { teamId: 'mutation-team-a', organizationId: org.id },
  null,
);
await compare('/organization/create-team', { name: 2, organizationId: null });
await compare('/organization/update-team', {
  teamId: 2,
  data: { name: 2, organizationId: null },
});
await compare('/organization/update-team', {
  teamId: 'mutation-team-a',
  data: { name: '' },
});
await compare('/organization/remove-team', { teamId: 2, organizationId: null });
for (const client of [clients[0], clients[4], clients[5]]) {
  await compare('/organization/create-team', { name: 'Denied' }, client);
  await compare(
    '/organization/update-team',
    { teamId: 'mutation-team-a', data: { name: 'Denied' } },
    client,
  );
  await compare(
    '/organization/remove-team',
    { teamId: 'mutation-team-a' },
    client,
  );
}
await compare(
  '/organization/create-team',
  { name: 'Foreign', organizationId: org.id },
  clients[4],
);
await compare(
  '/organization/create-team',
  { name: 'Unknown', organizationId: 'unknown' },
  clients[3],
);
await compare(
  '/organization/create-team',
  { name: 'Granted', id: 'injected', memberCount: 999, userId: users[0].id },
  clients[3],
);
await compare('/organization/create-team', { name: '' }, clients[3]);
await compare('/organization/list-teams');
for (const body of [
  { teamId: 'absent', data: {} },
  {
    teamId: 'mutation-team-a',
    data: { organizationId: organizations[1].id, name: 'Foreign' },
  },
  { teamId: reference.database.team[1].id, data: {} },
  { teamId: 'mutation-team-a', data: {} },
  {
    teamId: 'mutation-team-a',
    data: {
      name: 'Changed',
      id: 'injected',
      memberCount: 999,
      createdAt: 'injected',
      updatedAt: 'injected',
    },
  },
  {
    teamId: 'mutation-team-a',
    data: { name: 'Scoped', organizationId: org.id },
  },
])
  await compare('/organization/update-team', body, clients[3]);
await compare('/organization/remove-team', { teamId: 'absent' }, clients[3]);
await compare(
  '/organization/remove-team',
  { teamId: reference.database.team[1].id },
  clients[3],
);
await compare('/organization/remove-team', { teamId: team.id }, clients[3]);
await compare(
  '/organization/remove-team',
  { teamId: 'mutation-last-team', organizationId: 'mutation-last-org' },
  clients[3],
);
await compare(
  '/organization/remove-team',
  { teamId: 'mutation-team-a' },
  clients[3],
);
await compare('/organization/list-teams');
await compare('/organization/list-team-members', undefined, clients[2]);
await compare('/organization/set-active-team', { teamId: null }, clients[3]);
await compare('/organization/remove-team', { teamId: team.id }, clients[3]);
await compare(
  '/organization/remove-team',
  { teamId: 'mutation-team-b' },
  clients[3],
);
await compare('/organization/list-teams', undefined, clients[3]);
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
  `Better Auth team mutations: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
