// Actual trusted server-only organization member creation and storage effects.
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
for (let i = 0; i < 8; i++) {
  const client = reference.client();
  await atTime(start, () =>
    client.call('/sign-up/email', {
      name: `Addition ${i}`,
      firstName: 'Addition',
      lastName: `${i}`,
      email: `addition-${i}@fixture.test`,
      password: 'test-only-password',
    }),
  );
  clients.push({
    headers: new Headers({
      cookie: [...client.cookies]
        .map(([key, value]) => `${key}=${value}`)
        .join('; '),
    }),
    token: reference.database.session.at(-1).token,
    user: reference.database.user.at(-1),
    client,
  });
}
await atTime(start, () =>
  clients[0].client.call('/organization/create', {
    name: 'Addition A',
    slug: 'addition-a',
  }),
);
await atTime(start, () =>
  clients[5].client.call('/organization/create', {
    name: 'Addition B',
    slug: 'addition-b',
  }),
);
const org = reference.database.organization[0],
  other = reference.database.organization[1],
  team = reference.database.team[0];
const full = {
  id: 'full-organization',
  name: 'Full',
  slug: 'full-organization',
  createdAt: new RealDate(start),
};
reference.database.organization.push(full);
for (let i = 0; i < 100; i++)
  reference.database.member.push({
    id: `full-member-${i}`,
    organizationId: full.id,
    userId: `full-user-${i}`,
    role: 'member',
    createdAt: new RealDate(start),
  });
// A legacy existing team pair is reused without changing an inaccurate counter.
reference.database.teamMember.push({
  id: 'legacy-addition-pair',
  teamId: team.id,
  userId: clients[4].user.id,
  createdAt: new RealDate(start),
});
team.memberCount = 0;
reference.database.team.push({
  id: 'overcount-team',
  organizationId: org.id,
  name: 'Overcount',
  memberCount: 10,
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
const inputs = [{ op: 'seed', now: start, ...seed }],
  expected = [];
async function compare(body, client = null, now = start) {
  let result;
  try {
    result = {
      body: await atTime(now, () =>
        reference.auth.api.addMember({
          body,
          headers: client?.headers ?? new Headers(),
        }),
      ),
    };
  } catch (error) {
    result = { error: { code: error.body.code, message: error.body.message } };
  }
  // The typed Rust server API receives the string produced by Zod coercion.
  inputs.push({
    op: 'addMember',
    now,
    token: client?.token,
    body: { ...body, userId: String(body.userId) },
  });
  expected.push(JSON.parse(JSON.stringify(result)));
  inputs.push({ op: 'records', now, withOrganizations: true });
  expected.push(snapshot());
}
await compare({ userId: clients[1].user.id, role: 'member' });
await compare({ userId: '', role: 'member' }, clients[0]);
await compare({ userId: 'unknown', role: 'member', organizationId: org.id });
await compare({ userId: null, role: 'member', organizationId: org.id });
await compare({ userId: 123, role: 'member', organizationId: org.id });
await compare({
  userId: clients[1].user.id,
  role: 'member',
  organizationId: 'unknown',
});
await compare({
  userId: clients[1].user.id,
  role: 'member',
  organizationId: org.id,
  teamId: 'unknown',
});
await compare({
  userId: clients[1].user.id,
  role: 'member',
  organizationId: org.id,
  teamId: reference.database.team[1].id,
});
await compare({
  userId: clients[1].user.id,
  role: 'member',
  organizationId: full.id,
});
await compare({
  userId: clients[1].user.id,
  role: ['member', ' viewer ', 'member'],
  organizationId: org.id,
  teamId: team.id,
});
await compare({
  userId: clients[1].user.id,
  role: 'owner',
  organizationId: org.id,
  teamId: 'unknown',
});
await compare({ userId: clients[2].user.id, role: [], organizationId: org.id });
await compare({
  userId: clients[3].user.id,
  role: 'unknown, owner ',
  organizationId: org.id,
  teamId: '',
});
await compare({
  userId: clients[4].user.id,
  role: 'member',
  organizationId: org.id,
  teamId: team.id,
});
await compare(
  {
    userId: clients[6].user.id,
    role: 'viewer',
    organizationId: org.id,
    teamId: 'overcount-team',
  },
  clients[5],
);
// An authenticated non-member may use their selected org only as server scope.
await compare(
  { userId: clients[6].user.id, role: '', organizationId: '' },
  clients[5],
);
await compare(
  { userId: clients[7].user.id, role: 'member' },
  clients[0],
  start + 3600000,
);
await compare(
  { userId: 'unknown', role: 'member', organizationId: org.id },
  clients[0],
  start + 86400001,
);
await compare(
  { userId: clients[7].user.id, role: 'member' },
  clients[0],
  start + 86400001,
);
await compare(
  { userId: clients[7].user.id, role: 'member', organizationId: other.id },
  clients[0],
  start + 86400001,
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
const actual = raw;
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
  `Better Auth server-only member creation: ${expected.length} actual API/storage comparisons passed`,
);
