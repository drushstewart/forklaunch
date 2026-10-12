// Actual invitation creation, resend, roles, teams and admission limits.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { betterAuthReference } from "./fixtures/rust_better_auth_reference.mjs";
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
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
    client.call("/sign-up/email", {
      name: `Member ${i}`,
      firstName: "Member",
      lastName: `${i}`,
      email: `member-${i}@fixture.test`,
      password: "test-only-password",
    }),
  );
}
await atTime(start, () =>
  clients[0].call("/organization/create", {
    name: "Members A",
    slug: "members-a",
  }),
);
await atTime(start, () =>
  clients[4].call("/organization/create", {
    name: "Members B",
    slug: "members-b",
  }),
);
const organizations = reference.database.organization,
  users = reference.database.user;
const team = reference.database.team[0],
  org = organizations[0];
for (const [index, role] of [
  [1, "owner"],
  [2, "member"],
  [3, "viewer"],
  [6, "viewer"],
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
users[0].privateField = "must-not-appear-in-joined-member";
reference.database.organizationRole.push({
  id: "member-manager-grant",
  organizationId: org.id,
  role: "viewer",
  permission: '{"member":["update","delete"]}',
  createdAt: new RealDate(start),
});
reference.database.organizationRole.push({
  id: "member-malformed-grant",
  organizationId: organizations[1].id,
  role: "owner",
  permission: "{",
  createdAt: new RealDate(start),
});
const members = Object.fromEntries(
  reference.database.member.map((member) => [member.userId, { ...member }]),
);
reference.database.organizationRole.find(
  (row) => row.organizationId === org.id && row.role === "viewer",
).permission = JSON.stringify({ invitation: ["create"] });
const fields = {
  users: "user",
  accounts: "account",
  sessions: "session",
  organizations: "organization",
  members: "member",
  teams: "team",
  teamMembers: "teamMember",
  invitations: "invitation",
  organizationRoles: "organizationRole",
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
reference.database.organizationRole.push({
  id: "custom-invitation-role",
  organizationId: org.id,
  role: "custom",
  permission: "{}",
  createdAt: new RealDate(start),
});
const seed = snapshot(),
  seededIds = new Set(
    Object.values(seed)
      .flat()
      .map((row) => row.id),
  );
const inputs = [{ op: "replace-seed", now: start, ...snapshot() }],
  expected = [];
const attributes = (cookies) =>
  cookies
    .map((cookie) => [cookie.split("=")[0], cookie.slice(cookie.indexOf(";"))])
    .sort();
async function compare(
  route,
  body,
  client = clients[0],
  method = body === undefined ? "GET" : "POST",
  now = start,
) {
  const request = new Request(`http://localhost:3000/api/auth${route}`, {
    method,
    headers: {
      "content-type": "application/json",
      origin: "http://localhost:3000",
      ...(client
        ? {
            cookie: [...client.cookies]
              .map(([key, value]) => `${key}=${value}`)
              .join("; "),
          }
        : {}),
    },
    ...(body === undefined ? {} : { body: body }),
  });
  const response = await atTime(now, () => reference.auth.handler(request));
  let value = await response.text();
  try {
    value = JSON.parse(value);
  } catch {}
  inputs.push({
    op: "request",
    now: now,
    path: route,
    method,
    headers: Object.fromEntries(request.headers),
    ...(body === undefined ? {} : { body: body }),
  });
  expected.push({
    status: response.status,
    body: value,
    cookies: attributes(response.headers.getSetCookie()),
  });
  inputs.push({ op: "records", now: now, withOrganizations: true });
  expected.push(snapshot());
}
let sequence = 0;
const invite = async (body, client = clients[3], now = start) =>
  compare(
    "/organization/invite-member",
    typeof body === "string"
      ? body
      : body === undefined
        ? undefined
        : JSON.stringify(body),
    client,
    "POST",
    now,
  );
for (const body of [
  undefined,
  "null",
  "{}",
  '{"email":null,"role":"member"}',
  '{"email":"target@fixture.test"}',
  '{"email":"target@fixture.test","role":null}',
  '{"email":"target@fixture.test","role":[null]}',
  '{"email":"target@fixture.test","role":"member","resend":null}',
  '{"email":"target@fixture.test","role":"member","teamId":null}',
]) {
  for (const client of [clients[3], null]) await invite(body, client);
}
for (const email of [
  "",
  "invalid",
  " target@fixture.test ",
  users[0].email,
  users[5].email,
])
  await invite({ email, role: "member" });
for (const role of [
  "member",
  "viewer",
  "owner",
  "custom",
  "unknown",
  "unknown,unknown",
  "",
  [],
  ["member", "viewer"],
  " member ",
  " owner ",
  ["member", "member"],
  "viewer,owner",
  "viewer, owner",
]) {
  for (const client of [clients[0], clients[3], null])
    await invite({ email: `role-${sequence++}@fixture.test`, role }, client);
}
for (const teamId of [
  undefined,
  "",
  [],
  [team.id],
  team.id,
  [team.id, team.id],
  [""],
  [team.id, "absent"],
  "absent",
  team.id + ",absent",
  reference.database.team[1].id,
]) {
  await invite({
    email: `team-${sequence++}@fixture.test`,
    role: "member",
    ...(teamId === undefined ? {} : { teamId }),
  });
}
for (const organizationId of ["", org.id, organizations[1].id, "absent"]) {
  for (const client of [clients[3], clients[4], clients[5]])
    await invite(
      {
        email: `scope-${sequence++}@fixture.test`,
        role: "member",
        organizationId,
      },
      client,
    );
}
const first = reference.database.invitation.find(
  (row) => row.email === users[5].email,
);
await invite({ email: first.email.toUpperCase(), role: "member" });
await invite(
  {
    email: first.email.toUpperCase(),
    role: "viewer",
    resend: true,
    teamId: "absent",
  },
  clients[3],
  start + 1000,
);
await invite({ email: first.email, role: "unknown", resend: true });
reference.database.invitation.length = 0;
for (let i = 0; i < 100; i++)
  reference.database.invitation.push({
    id: "limit-invite-" + i,
    email: `limit-${i}@fixture.test`,
    role: "member",
    organizationId: org.id,
    inviterId: users[0].id,
    status: "pending",
    expiresAt: new RealDate(start + 10000),
    createdAt: new RealDate(start),
    teamId: null,
  });
inputs.push({ op: "replace-seed", now: start, ...snapshot() });
expected.push({ ok: true });
await invite({ email: "over-limit@fixture.test", role: "member" });
await invite({ email: "limit-0@fixture.test", role: "member", resend: true });
reference.database.invitation[0].expiresAt = new RealDate(start);
reference.database.invitation.push({
  id: "late-pending-invite",
  email: "late@fixture.test",
  role: "member",
  organizationId: org.id,
  inviterId: users[0].id,
  status: "pending",
  expiresAt: new RealDate(start + 10000),
  createdAt: new RealDate(start),
  teamId: null,
});
inputs.push({ op: "replace-seed", now: start, ...snapshot() });
expected.push({ ok: true });
await invite({ email: "bounded-limit@fixture.test", role: "member" });
for (const method of ["GET", "HEAD", "OPTIONS"])
  await compare("/organization/invite-member", undefined, clients[3], method);
const run = spawnSync(
  "cargo",
  [
    "run",
    "--quiet",
    "--manifest-path",
    "blueprint-rs/Cargo.toml",
    "-p",
    "forklaunch-iam-better-auth",
    "--features",
    "http",
    "--example",
    "http_parity",
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: "0" },
    input: inputs.map((input) => JSON.stringify(input)).join("\n") + "\n",
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  },
);
assert.equal(run.status, 0, run.stderr);
const raw = run.stdout
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line))
  .slice(1);
assert.equal(raw.length, expected.length);
const actual = raw.map((value, i) =>
  inputs[i + 1].op === "request"
    ? {
        status: value.status,
        body: value.body,
        cookies: attributes(value.headers["set-cookie"] ?? []),
      }
    : value,
);

function normalize(value, ids = new Map()) {
  if (Array.isArray(value)) return value.map((item) => normalize(item, ids));
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, item]) => {
          if (key === "membershipKey") {
            assert.equal(
              item,
              createHash("sha256")
                .update(JSON.stringify([value.teamId, value.userId]))
                .digest("base64url"),
            );
            return [key, "[validated team membership key]"];
          }
          return [key, normalize(item, ids)];
        }),
    );
  if (
    typeof value === "string" &&
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
      diff: error.message.slice(0, 1500),
    });
  }
}
assert.deepEqual(mismatches.slice(0, 10), []);
console.log(
  `Better Auth invitation creation: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
