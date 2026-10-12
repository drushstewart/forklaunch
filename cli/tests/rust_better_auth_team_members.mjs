// Actual team membership mutations, Zod coercion and keyed counter lifecycle.
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
reference.database.organizationRole.find(
  (row) => row.organizationId === org.id && row.role === 'viewer',
).permission = JSON.stringify({ member: ['update', 'delete'] });
const values = [
  undefined,
  null,
  0,
  -0,
  1,
  1.0,
  1e21,
  1e-7,
  9007199254740992,
  true,
  false,
  '',
  [],
  ['a', 'b'],
  {},
  [null],
  ['a', [null, 'b']],
];
for (const [i, value] of values.entries()) {
  const id = String(value);
  if (!id || reference.database.user.some((row) => row.id === id)) continue;
  reference.database.user.push({
    id,
    email: `coercion-${i}@fixture.test`,
    name: 'Coercion User',
  });
  reference.database.member.push({
    id: `coercion-member-${i}`,
    organizationId: org.id,
    userId: id,
    role: 'member',
    createdAt: new RealDate(start),
  });
}
reference.database.team.push({
  id: 'membership-team',
  organizationId: org.id,
  name: 'Membership',
  memberCount: 0,
  createdAt: new RealDate(start),
});
// A corrupt undercount is reconciled for addition, and retained on release if
// fewer stored seats exist than duplicate legacy rows being removed.
reference.database.teamMember.push({
  id: 'legacy-team-pair-1',
  teamId: 'membership-team',
  userId: users[1].id,
  createdAt: new RealDate(start),
});
reference.database.teamMember.push({
  id: 'legacy-team-pair-2',
  teamId: 'membership-team',
  userId: users[1].id,
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
  '/organization/add-team-member',
  '/organization/remove-team-member',
]) {
  for (const body of [
    undefined,
    null,
    [],
    false,
    {},
    { teamId: 2, organizationId: null },
    { teamId: 2, userId: { toString: null }, organizationId: null },
  ])
    await compare(route, body, clients[0], 'POST');
  await compare(
    route,
    { teamId: 'membership-team', userId: users[2].id },
    null,
  );
  for (const client of [clients[0], clients[4], clients[5]])
    await compare(
      route,
      { teamId: 'membership-team', userId: users[2].id },
      client,
    );
  await compare(
    route,
    { teamId: 'membership-team', userId: users[2].id, organizationId: org.id },
    clients[4],
  );
  for (const userId of [
    'absent-user',
    users[4].id,
    { toString: 1 },
    [{ toString: 1 }],
    { valueOf: 1 },
  ])
    await compare(route, { teamId: 'membership-team', userId }, clients[3]);
  await compare(route, { teamId: 'absent', userId: users[2].id }, clients[3]);
  await compare(
    route,
    { teamId: reference.database.team[1].id, userId: users[2].id },
    clients[3],
  );
}
// Removing duplicate legacy pairs with a zero count leaves the undercount alone.
await compare(
  '/organization/remove-team-member',
  { teamId: 'membership-team', userId: users[1].id },
  clients[3],
);
await compare(
  '/organization/add-team-member',
  { teamId: 'membership-team', userId: users[2].id },
  clients[3],
);
await compare(
  '/organization/add-team-member',
  { teamId: 'membership-team', userId: users[2].id },
  clients[3],
);
await compare(
  '/organization/list-team-members?teamId=membership-team',
  undefined,
  clients[2],
);
for (const userId of values) {
  await compare(
    '/organization/add-team-member',
    { teamId: 'membership-team', ...(userId === undefined ? {} : { userId }) },
    clients[3],
  );
  await compare(
    '/organization/remove-team-member',
    { teamId: 'membership-team', ...(userId === undefined ? {} : { userId }) },
    clients[3],
  );
}
await compare(
  '/organization/set-active-team',
  { teamId: 'membership-team' },
  clients[2],
);
await compare(
  '/organization/remove-team-member',
  { teamId: 'membership-team', userId: users[2].id },
  clients[3],
);
await compare('/organization/list-team-members', undefined, clients[2]);
await compare(
  '/organization/remove-team-member',
  { teamId: 'membership-team', userId: users[2].id },
  clients[3],
);
await compare(
  '/organization/add-team-member',
  { teamId: 'membership-team', userId: users[2].id },
  clients[3],
);
await compare('/organization/list-team-members', undefined, clients[2]);
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
  `Better Auth team membership mutations: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
