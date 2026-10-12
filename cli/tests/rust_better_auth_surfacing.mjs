// Compare application surfacing with the actual configured TypeScript service.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const RealDate = Date;
const now = Math.floor(Date.now() / 1000) * 1000;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [now]));
  }
  static now() {
    return now;
  }
};
const reference = await betterAuthReference(root);
const child = spawn(
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
    'http_parity'
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    stdio: ['pipe', 'pipe', 'inherit']
  }
);
const pending = [];
createInterface({ input: child.stdout }).on('line', (line) =>
  pending.shift().resolve(JSON.parse(line))
);
const completion = new Promise((resolve, reject) => {
  child.once('error', reject);
  child.once('exit', (code) => {
    if (code === 0) resolve();
    else reject(new Error(`Public bridge exited ${code}`));
    for (const p of pending.splice(0))
      p.reject(new Error('Public bridge closed'));
  });
});
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
const stamp = (offset) => new RealDate(now + offset).toISOString();
const userId = 'surface-user';
const session = (id, org, created = 0, expiry = 10000, owner = userId) => ({
  id,
  userId: owner,
  activeOrganizationId: org,
  createdAt: stamp(created),
  expiresAt: stamp(expiry)
});
const member = (id, org, role, owner = userId) => ({
  id,
  userId: owner,
  organizationId: org,
  role
});
const grant = (id, org, role, permission) => ({
  id,
  organizationId: org,
  role,
  permission
});
let checks = 0;
async function compare(sessions, members = [], roles = []) {
  reference.database.session.splice(0, Infinity, ...sessions);
  reference.database.member.splice(0, Infinity, ...members);
  reference.database.organizationRole.splice(0, Infinity, ...roles);
  await native({
    op: 'replace-seed',
    now,
    sessions,
    members,
    organizationRoles: roles
  });
  const expected = JSON.parse(
    JSON.stringify({
      role: await reference.surfacing.surfaceRole(userId),
      permissions: await reference.surfacing.surfacePermissions(userId)
    })
  );
  const actual = await native({ op: 'surface', now, userId });
  assert.deepEqual(
    actual,
    expected,
    JSON.stringify({ sessions, members, roles })
  );
  checks++;
}
try {
  await compare([]);
  const members = [member('m1', 'one', 'owner'), member('m2', 'two', 'admin')];
  const roles = [
    grant('r1', 'one', 'owner', '{"platform":["platform:read"]}'),
    grant('r2', 'one', 'owner', 'opaque raw grant'),
    grant('r3', 'two', 'admin', 'two grant'),
    grant('r4', 'one', 'admin', 'wrong-role'),
    grant('r5', 'other', 'owner', 'wrong-org')
  ];
  await compare([session('s1', 'one')], members, roles);
  await compare(
    [session('s1', 'one'), session('s2', 'two', 1)],
    members,
    roles
  );
  await compare(
    [session('s1', 'one'), session('s2', 'two', -1)],
    members,
    roles
  );
  await compare(
    [session('s1', 'one'), session('s2', 'two', 0)],
    members,
    roles
  );
  await compare(
    [session('s2', 'two', 0), session('s1', 'one', 0)],
    members,
    roles
  );
  for (const org of [null, '', false, 0, 'missing', 'one']) {
    for (const expiry of [-1, 0, 1]) {
      await compare(
        [session('old', 'one', -10), session('new', org, 10, expiry)],
        members,
        roles
      );
    }
  }
  await compare(
    [session('one', 'one'), session('foreign', 'two', 10, 10000, 'other')],
    members,
    roles
  );
  await compare(
    [session('one', 'one')],
    [member('first', 'one', 'admin'), ...members],
    roles
  );
  for (const role of [
    null,
    '',
    false,
    0,
    'owner,admin',
    'OWNER',
    'member',
    'owner'
  ]) {
    await compare([session('one', 'one')], [member('one', 'one', role)], roles);
  }
  await compare([session('one', 'one')], members, [
    ...roles,
    grant('duplicate', 'one', 'owner', 'opaque raw grant')
  ]);
  await compare([session('one', 'one')], members, [
    grant('empty', 'one', 'owner', ''),
    grant('null', 'one', 'owner', null),
    { id: 'omitted', organizationId: 'one', role: 'owner' }
  ]);
  await compare(
    [
      { ...session('one', 'one'), expiresAt: 'invalid-date' },
      session('two', 'two', -1)
    ],
    members,
    roles
  );
  console.log(
    `Configured Better Auth application surfacing: ${checks} actual service result comparisons passed`
  );
} finally {
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
}
