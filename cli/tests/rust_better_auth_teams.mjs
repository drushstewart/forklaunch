// Actual team reads, cross-user scope, internal projections and session selection.
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
// Retained team membership without organization membership exercises the distinct
// selection/read contracts rather than assuming all routes share one guard.
reference.database.member.splice(
  reference.database.member.indexOf(members[users[6].id]),
  1,
);
reference.database.team.push({
  id: 'team-empty-a',
  organizationId: org.id,
  name: 'Empty A',
  memberCount: 0,
  createdAt: new RealDate(start),
});
reference.database.teamMember.push({
  id: 'team-cross-membership',
  teamId: reference.database.team[1].id,
  userId: users[2].id,
  createdAt: new RealDate(start),
});
reference.database.member.push({
  id: 'team-orphan-member',
  organizationId: org.id,
  userId: 'absent-user',
  role: 'member',
  createdAt: new RealDate(start),
});
await atTime(start, () =>
  clients[6].call('/sign-in/email', {
    email: users[6].email,
    password: 'test-only-password',
    rememberMe: false,
  }),
);
reference.database.session.at(-1).activeOrganizationId = org.id;
reference.database.session.at(-1).activeTeamId = team.id;
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
  now = start,
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
  '/organization/list-teams',
  '/organization/list-user-teams',
  '/organization/list-team-members',
]) {
  await compare(route, undefined, null);
  for (const client of clients) await compare(route, undefined, client);
}
for (const body of [
  undefined,
  null,
  [],
  false,
  { teamId: 1 },
  { teamId: [] },
  { teamId: false },
])
  await compare('/organization/set-active-team', body, clients[0], 'POST');
await compare('/organization/set-active-team', { teamId: team.id }, null);
for (const organizationId of ['', org.id, organizations[1].id, 'absent']) {
  for (const client of [clients[0], clients[4], clients[5], clients[6]])
    await compare(
      '/organization/list-teams?organizationId=' + organizationId,
      undefined,
      client,
    );
}
await compare('/organization/list-teams?organizationId=a&organizationId=b');
await compare('/organization/list-team-members?teamId=a&teamId=b');
await compare(
  '/organization/list-user-teams?userId=a&userId=b&organizationId=a&organizationId=b',
);
for (const teamId of [
  '',
  team.id,
  reference.database.team[1].id,
  'team-empty-a',
  'absent',
]) {
  for (const client of [
    clients[0],
    clients[2],
    clients[4],
    clients[5],
    clients[6],
  ])
    await compare(
      '/organization/list-team-members?teamId=' + teamId,
      undefined,
      client,
    );
}
for (const client of [
  clients[0],
  clients[3],
  clients[4],
  clients[5],
  clients[6],
]) {
  for (const [target, organizationId] of [
    [undefined, undefined],
    [users[2].id, undefined],
    [users[2].id, org.id],
    [users[4].id, org.id],
    ['absent-user', org.id],
    [users[2].id, organizations[1].id],
    [users[2].id, 'absent'],
    [undefined, org.id],
    [undefined, organizations[1].id],
  ]) {
    const query = new URLSearchParams();
    if (target !== undefined) query.set('userId', target);
    if (organizationId !== undefined)
      query.set('organizationId', organizationId);
    await compare('/organization/list-user-teams?' + query, undefined, client);
  }
}
for (const client of [clients[0], clients[5], clients[6]]) {
  for (const body of [
    {},
    { teamId: '' },
    { teamId: null },
    {},
    { teamId: team.id },
    { teamId: team.id },
    { teamId: 'team-empty-a' },
    { teamId: reference.database.team[1].id },
    { teamId: 'absent' },
    { teamId: null },
    { teamId: null },
  ])
    await compare('/organization/set-active-team', body, client);
}
await compare('/organization/list-team-members');
await compare('/organization/list-user-teams');
await compare(
  '/organization/set-active-team',
  { teamId: team.id },
  clients[0],
  'POST',
  start + 1000,
);
await compare(
  '/organization/set-active-team',
  { teamId: null },
  clients[0],
  'POST',
  start + 2000,
);
await compare(
  '/organization/set-active-team',
  { teamId: team.id },
  clients[6],
  'POST',
  start + 3000,
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
  `Better Auth team reads and selection: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
