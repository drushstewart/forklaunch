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
  '/organization/leave',
  '/organization/remove-member',
  '/organization/update-member-role',
])
  for (const body of [undefined, null, [], {}, false])
    await compare(route, body, clients[0], 'POST');
for (const route of [
  '/organization/get-active-member',
  '/organization/get-active-member-role',
])
  await compare(route, undefined, null);
await compare('/organization/leave', { organizationId: org.id }, null);
await compare(
  '/organization/remove-member',
  { memberIdOrEmail: members[users[2].id].id },
  null,
);
await compare(
  '/organization/update-member-role',
  { memberId: members[users[2].id].id, role: 'viewer' },
  null,
);
await compare('/organization/update-member-role', {
  role: [1],
  memberId: 2,
  organizationId: null,
});
for (const client of clients)
  await compare('/organization/get-active-member', undefined, client);
for (const query of [
  '',
  `?userId=${users[2].id}`,
  `?organizationId=${org.id}&userId=${users[1].id}`,
  `?organizationSlug=${org.slug}&organizationId=absent`,
  `?userId=${users[4].id}`,
  '?organizationId=absent',
  '?organizationSlug=absent',
  '?userId=a&userId=b&organizationId=a&organizationId=b&organizationSlug=a&organizationSlug=b',
])
  await compare('/organization/get-active-member-role' + query);
await compare(
  `/organization/get-active-member?organizationId=${organizations[1].id}`,
);
for (const role of [
  '',
  [],
  ['', ' '],
  ' , ',
  ['unknown', 'unknown2', 'unknown'],
  '\ufeffviewer\ufeff',
  'owner,viewer',
  ['member, editor', 'system'],
])
  await compare('/organization/update-member-role', {
    memberId: members[users[2].id].id,
    role,
  });
for (const role of ['', [], [' '], 'viewer'])
  await compare(
    '/organization/update-member-role',
    { memberId: members[users[2].id].id, role },
    clients[5],
  );
await compare('/organization/update-member-role', {
  memberId: 'unknown',
  role: 'viewer',
});
await compare('/organization/update-member-role', {
  memberId: members[users[4].id].id,
  role: 'viewer',
});
await compare(
  '/organization/update-member-role',
  { memberId: members[users[0].id].id, role: 'viewer' },
  clients[3],
);
await compare(
  '/organization/update-member-role',
  { memberId: members[users[2].id].id, role: 'owner' },
  clients[3],
);
await compare(
  '/organization/update-member-role',
  { memberId: members[users[2].id].id, role: 'editor' },
  clients[3],
);
await compare(
  '/organization/update-member-role',
  { memberId: members[users[4].id].id, role: 'owner' },
  clients[4],
);
await compare('/organization/remove-member', {
  memberIdOrEmail: members[users[2].id].id,
});
await compare(
  '/organization/remove-member',
  { memberIdOrEmail: 'unknown' },
  clients[3],
);
await compare(
  '/organization/remove-member',
  { memberIdOrEmail: members[users[4].id].id },
  clients[3],
);
await compare(
  '/organization/remove-member',
  { memberIdOrEmail: members[users[0].id].id },
  clients[3],
);
await compare(
  '/organization/remove-member',
  { memberIdOrEmail: members[users[2].id].id },
  clients[3],
);
await compare('/organization/get-active-member', undefined, clients[2]);
await compare('/organization/leave', { organizationId: org.id }, clients[2]);
await compare(
  '/organization/remove-member',
  { memberIdOrEmail: users[6].email.toUpperCase() },
  clients[3],
);
await compare(
  '/organization/remove-member',
  { memberIdOrEmail: members[users[3].id].id },
  clients[3],
);
await compare('/organization/get-active-member-role', undefined, clients[3]);
await compare('/organization/leave', { organizationId: 'unknown' });
await compare('/organization/leave', { organizationId: org.id }, clients[1]);
await compare('/organization/leave', { organizationId: org.id });
await compare('/organization/remove-member', {
  memberIdOrEmail: members[users[0].id].id,
});
await compare('/organization/update-member-role', {
  memberId: members[users[0].id].id,
  role: 'viewer',
});
await compare('/organization/update-member-role', {
  memberId: members[users[0].id].id,
  role: 'owner,owner',
});
await compare('/organization/get-active-member-role');
await compare('/organization/get-full-organization');
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
  `Better Auth member lifecycle: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
