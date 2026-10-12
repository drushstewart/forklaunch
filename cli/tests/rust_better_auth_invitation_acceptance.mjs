// Actual invitation acceptance, team enrollment, cookies and rollback.
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
).permission = JSON.stringify({ organization: ["update"], member: ["update"] });
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
users[0].emailVerified = true;
users[5].emailVerified = true;
await atTime(start, () =>
  clients[5].call("/sign-in/email", {
    email: users[5].email,
    password: "test-only-password",
    rememberMe: false,
  }),
);
users[4].emailVerified = true;
for (const [i, status, expiry, organizationId, inviterId, email] of [
  [0, "pending", 1, org.id, users[1].id, users[0].email],
  [1, "pending", 0, org.id, users[1].id, users[0].email],
  [2, "pending", -1, org.id, users[1].id, users[0].email],
  [3, "accepted", 1, org.id, users[1].id, users[0].email],
  [4, "rejected", 1, org.id, users[1].id, users[0].email],
  [5, "pending", 1, "missing-org", users[1].id, users[0].email],
  [6, "pending", 1, org.id, "missing-inviter", users[0].email],
  [7, "pending", 1, org.id, users[5].id, users[0].email],
  [8, "pending", 1, org.id, users[1].id, users[3].email],
  [9, "pending", 1, org.id, users[1].id, users[0].email.toUpperCase()],
]) {
  reference.database.invitation.push({
    id: "invite-" + i,
    email,
    role: "member",
    organizationId,
    inviterId,
    status,
    expiresAt: new RealDate(start + expiry),
    createdAt: new RealDate(start),
    teamId: null,
  });
}
for (let i = 0; i < 105; i++)
  reference.database.invitation.push({
    id: "bulk-" + i,
    email: users[0].email,
    role: "viewer",
    organizationId: org.id,
    inviterId: users[1].id,
    status: i < 90 ? "accepted" : "pending",
    expiresAt: new RealDate(start - 1),
    createdAt: new RealDate(start),
    teamId: null,
  });
reference.database.team.push({
  id: "accept-second-team",
  organizationId: org.id,
  name: "Second",
  memberCount: 0,
  createdAt: new RealDate(start),
});
reference.database.team.push({
  id: "accept-rollback-team",
  organizationId: org.id,
  name: "Rollback",
  memberCount: 0,
  createdAt: new RealDate(start),
});
for (const [i, teamId, expiry, role] of [
  [10, null, 10000, "member"],
  [11, team.id, 10000, "viewer"],
  [12, team.id + ",accept-second-team", 10000, "member"],
  [13, team.id + "," + team.id, 10000, "member"],
  [14, "accept-rollback-team,absent", 10000, "member"],
  [15, reference.database.team[1].id, 10000, "member"],
  [16, "", 10000, "member"],
  [17, ",", 10000, "member"],
  [18, null, 10000, "custom, unknown"],
  [19, team.id, 1000, "member"],
  [20, null, -1, "member"],
])
  reference.database.invitation.push({
    id: "accept-" + i,
    email: users[5].email,
    role,
    organizationId: org.id,
    inviterId: users[0].id,
    status: "pending",
    expiresAt: new RealDate(start + expiry),
    createdAt: new RealDate(start),
    teamId,
  });
const seed = snapshot(),
  seededIds = new Set(
    Object.values(seed)
      .flat()
      .map((row) => row.id),
  );
const inputs = [{ op: "seed", now: start, ...snapshot() }],
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
const accept = async (body, client = clients[5], now = start) =>
  compare(
    "/organization/accept-invitation",
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
  '{"invitationId":null}',
  '{"invitationId":1}',
  '{"invitationId":""}',
  '{"invitationId":"absent"}',
]) {
  for (const client of [clients[0], null]) await accept(body, client);
}
for (const row of reference.database.invitation.slice(0, 10)) {
  for (const client of [clients[4], clients[3], clients[0], clients[0], null])
    await accept({ invitationId: row.id }, client);
}
for (let i = 10; i <= 20; i++) {
  await accept({ invitationId: "accept-" + i }, clients[5], start + 1000);
  await accept({ invitationId: "accept-" + i }, clients[5], start + 1000);
}
for (const method of ["GET", "HEAD", "OPTIONS"])
  await compare(
    "/organization/accept-invitation",
    undefined,
    clients[0],
    method,
  );
let count = reference.database.member.filter(
  (row) => row.organizationId === org.id,
).length;
for (; count < 100; count++)
  reference.database.member.push({
    id: "accept-limit-member-" + count,
    organizationId: org.id,
    userId: users[0].id,
    role: "member",
    createdAt: new RealDate(start),
  });
reference.database.invitation.push({
  id: "accept-limit-invite",
  email: users[5].email,
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
await accept({ invitationId: "accept-limit-invite" });
reference.database.member.splice(
  reference.database.member.findIndex((row) =>
    row.id.startsWith("accept-limit-member-"),
  ),
  1,
);
inputs.push({ op: "replace-seed", now: start, ...snapshot() });
expected.push({ ok: true });
await accept({ invitationId: "accept-limit-invite" });
await accept({ invitationId: "accept-limit-invite" });
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

function normalize(value, ids = new Map(), known = seededIds) {
  if (Array.isArray(value))
    return value.map((item) => normalize(item, ids, known));
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
          return [key, normalize(item, ids, known)];
        }),
    );
  if (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f-]{27}$/.test(value) &&
    !known.has(value)
  ) {
    if (!ids.has(value)) ids.set(value, `[generated:${ids.size}]`);
    return ids.get(value);
  }
  return value;
}
function normalizeTimeline(values) {
  let ids = new Map(),
    known = new Set(seededIds);
  return values.map((value, i) => {
    const input = inputs[i + 1];
    if (input.op === "replace-seed") {
      ids = new Map();
      known = new Set(
        Object.values(input)
          .filter(Array.isArray)
          .flat()
          .map((row) => row.id),
      );
    }
    return normalize(value, ids, known);
  });
}
const normalizedActual = normalizeTimeline(actual),
  normalizedExpected = normalizeTimeline(expected);
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
  `Better Auth invitation acceptance: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
