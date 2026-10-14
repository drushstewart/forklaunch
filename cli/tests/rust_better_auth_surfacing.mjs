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
const userId = 'a1000000-0000-4000-8000-000000000001';
const org1 = 'a2000000-0000-4000-8000-000000000001';
const org2 = 'a2000000-0000-4000-8000-000000000002';
const session1 = 'a3000000-0000-4000-8000-000000000001';
const session2 = 'a3000000-0000-4000-8000-000000000002';
const scope = { userId, organizationId: org1, sessionId: session1 };
const session = (id, org, created = 0, expiry = 10000, owner = userId) => ({ id,userId:owner,activeOrganizationId:org,createdAt:stamp(created),expiresAt:stamp(expiry) });
const member = (id, org, role, owner = userId) => ({id,userId:owner,organizationId:org,role});
const grant = (id, org, role, permission) => ({id,organizationId:org,role,permission});
let checks = 0;
async function compare(sessions, members = [], roles = [], selected = scope) {
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
      role: await reference.surfacing.surfaceRole(selected),
      permissions: await reference.surfacing.surfacePermissions(selected)
    })
  );
  const actual = await native({ op: 'surface', now, scope:selected });
  assert.deepEqual(
    actual,
    expected,
    JSON.stringify({ sessions, members, roles })
  );
  checks++;
}
try {
  await compare([]);
  const members = [member('m1', org1, 'owner'),member('m2', org2, 'viewer')];
  const sessions = [session(session1,org1),session(session2,org2,100)];
  const roles = [grant('r1',org1,'owner','ignored builtin override'),grant('r2',org1,'custom','opaque raw grant'),grant('r3',org2,'custom','wrong organization')];
  await compare(sessions,members,roles);
  await compare(sessions,members,roles,{...scope,sessionId:session1.toUpperCase()});
  await compare(sessions,members,roles,{...scope,organizationId:org1.toUpperCase()});
  await compare(sessions,[...members,member("uppercase-user",org1,"viewer",userId.toUpperCase())],roles,{...scope,userId:userId.toUpperCase()});
  await compare(sessions,members,roles,{...scope,organizationId:org2,sessionId:session2});
  for (const field of ['userId','organizationId','sessionId']) {
    for (const value of [null,'',42,false,'not-a-uuid','00000000-0000-0000-0000-000000000000']) {
      await compare(sessions,members,roles,{...scope,[field]:value});
    }
  }
  for (const selected of [null,{},userId]) await compare(sessions,members,roles,selected);
  for (const expiry of [-1,0,1]) await compare([session(session1,org1,0,expiry),session(session2,org2)],members,roles);
  for (const org of [null,'',org2]) await compare([session(session1,org),session(session2,org1)],members,roles);
  for (const owner of ['other',null]) await compare([session(session1,org1,0,10000,owner),session(session2,org1)],members,roles);
  for (const role of ['owner','admin','editor','viewer','system','custom','owner,admin','OWNER','member','',null,false,0]) {
    await compare(sessions,[member('m1',org1,role)],roles);
  }
  await compare(sessions,[],roles);
  await compare(sessions,[member('m1',org2,'owner')],roles);
  await compare(sessions,[member('m1',org1,'custom')],[...roles,grant('duplicate',org1,'custom','opaque raw grant')]);
  await compare([{...session(session1,org1),expiresAt:'invalid'},session(session2,org1)],members,roles);
  console.log(
    `Configured Better Auth application surfacing: ${checks} actual service result comparisons passed`
  );
} finally {
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
}
