// Actual invitation rejection/cancellation and persisted status behavior.
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
reference.database.organizationRole.find(
  (row) => row.organizationId === org.id && row.role === "viewer",
).permission = JSON.stringify({ invitation: ["cancel"] });
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
  const response = await atTime(start, () => reference.auth.handler(request));
  let value = await response.text();
  try {
    value = JSON.parse(value);
  } catch {}
  inputs.push({
    op: "request",
    now: start,
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
  inputs.push({ op: "records", now: start, withOrganizations: true });
  expected.push(snapshot());
}
for (const route of [
  "/organization/reject-invitation",
  "/organization/cancel-invitation",
]) {
  for (const body of [
    undefined,
    "null",
    "{}",
    '{"invitationId":null}',
    '{"invitationId":12}',
    '{"invitationId":""}',
    '{"invitationId":"absent"}',
  ]) {
    for (const client of [clients[0], clients[3], null])
      await compare(route, body, client, "POST");
  }
  for (const row of reference.database.invitation.slice(0, 11)) {
    for (const client of [
      clients[4],
      clients[0],
      clients[3],
      clients[0],
      null,
    ]) {
      await compare(
        route,
        JSON.stringify({ invitationId: row.id, ignored: "stripped" }),
        client,
        "POST",
      );
    }
  }
}
await compare("/organization/list-invitations?organizationId=" + org.id);
for (const route of ["reject-invitation", "cancel-invitation"]) {
  for (const method of ["GET", "HEAD", "OPTIONS"])
    await compare("/organization/" + route, undefined, clients[0], method);
}
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
  `Better Auth invitation rejection/cancellation: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
