// Actual scoped member listing, schema, filters, slicing and Unicode ordering.
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
const roles = [
  'a',
  'A',
  'á',
  'ä',
  'e',
  'é',
  'e\u0301',
  'Z',
  'z',
  'ß',
  'ss',
  'œ',
  'oe',
  'Æ',
  'ae',
  '😀',
  '😃',
  '中',
  '文',
  'éclair',
  'Éclair',
  'item2',
  'item10',
  ' owner',
  'owner',
  'owner,member',
  'viewer',
  'a-b',
  'ab',
  'a_b',
  'a b',
  'a.b',
  'a!b',
  '𐀀',
  '\ufeffa',
  'å',
  'Å',
  'ñ',
  'n',
  'ø',
  'o',
  'İ',
  'i',
  'ı',
  'Ω',
  'ω',
  'Σ',
  'σ',
  'ς',
  '\t',
  '\n',
  '',
];
for (let i = 0; i < 128; i++) {
  const id = `listing-user-${i}`;
  reference.database.user.push({
    id,
    email: `listing-${i}@fixture.test`,
    name: `List ${i}`,
    privateField: 'not-public',
  });
  reference.database.member.push({
    id: `listing-member-${i}`,
    userId: id,
    organizationId: org.id,
    role: roles[i % roles.length],
    createdAt: new RealDate(start + (i % 7) * 1000),
  });
}
reference.database.member.push({
  id: 'listing-orphan',
  organizationId: org.id,
  userId: 'absent-user',
  role: 'orphan',
  createdAt: new RealDate(start + 9000),
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
await compare('/organization/list-members', undefined, null);
await compare('/organization/list-members', undefined, clients[5]);
await compare(
  `/organization/list-members?organizationId=${org.id}`,
  undefined,
  clients[4],
);
const queries = [
  '',
  '?organizationId=absent',
  '?organizationSlug=absent',
  `?organizationSlug=${org.slug}&organizationId=absent`,
  '?sortDirection=invalid&filterOperator=invalid',
  '?limit=a&limit=b&offset=a&offset=b&sortBy=a&sortBy=b&sortDirection=a&sortDirection=b&filterField=a&filterField=b&organizationId=a&organizationId=b&organizationSlug=a&organizationSlug=b',
];
for (const limit of [
  '',
  '0',
  '-0',
  '1',
  '2',
  '100',
  '200',
  '-1',
  '-2',
  '-999',
  '1.9',
  '-1.9',
  'Infinity',
  '-Infinity',
  'NaN',
  '1e2',
  '1e309',
  '0x10',
  '0b10',
  '0o10',
  '+0x10',
  '0x1p2',
  ' 2 ',
  '2junk',
  'inf',
  '2_0',
  '\ufeff2\ufeff',
  '\u00852',
])
  queries.push('?limit=' + encodeURIComponent(limit));
for (const offset of [
  '',
  '0',
  '1',
  '3',
  '100',
  '999',
  '-1',
  '-3',
  '-999',
  '1.9',
  '-1.9',
  'Infinity',
  '-Infinity',
  'NaN',
  '0x10',
  'bad',
])
  queries.push('?limit=2&offset=' + encodeURIComponent(offset));
for (const sortBy of [
  '',
  'role',
  'id',
  'userId',
  'organizationId',
  'createdAt',
  'unknown',
])
  for (const sortDirection of ['asc', 'desc'])
    queries.push(
      '?limit=-1&filterField=role&filterOperator=ne&filterValue=orphan&sortBy=' +
        sortBy +
        '&sortDirection=' +
        sortDirection,
    );
for (const [field, value] of [
  ['role', 'owner'],
  ['role', 'a'],
  ['userId', users[0].id],
  ['id', 'listing-member-10'],
  ['organizationId', org.id],
  ['createdAt', String(start)],
  ['createdAt', new RealDate(start).toISOString()],
  ['unknown', 'x'],
]) {
  for (const operator of [
    'eq',
    'ne',
    'lt',
    'lte',
    'gt',
    'gte',
    'contains',
    'starts_with',
    'ends_with',
    'in',
    'not_in',
  ])
    queries.push(
      '?limit=5&filterField=' +
        field +
        '&filterValue=' +
        encodeURIComponent(value) +
        '&filterOperator=' +
        operator,
    );
}
for (const operator of [
  'in',
  'not_in',
  'eq',
  'ne',
  'contains',
  'starts_with',
  'ends_with',
])
  queries.push(
    '?limit=4&filterField=role&filterValue=owner&filterValue=viewer&filterOperator=' +
      operator,
  );
queries.push(
  '?filterField=role',
  '?filterField=role&filterOperator=ne&limit=2',
  '?filterField=&filterOperator=in',
  '?filterField=role&filterValue=nonexistent&sortBy=unknown',
  '?filterField=id&filterValue=listing-member-1&sortBy=unknown',
  '?limit=-1&offset=1',
  '?limit=2&offset=-3',
  '?filterField=role&filterValue=viewer&offset=1&limit=1',
);
for (const query of queries)
  await compare('/organization/list-members' + query);
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
  `Better Auth member listing: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
