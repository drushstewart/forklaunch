// Actual dynamic role deletion, assigned-member guards and adapter limits.
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
).permission = JSON.stringify({ ac: ["read", "delete"] });
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
for (const [id, role, permission] of [
  ["mixed-role", "MiXeD", '{"platform":["platform:read"]}'],
  ["empty-role", "", "{}"],
  ["spaced-role", " spaced ", '{"platform":[]}'],
])
  reference.database.organizationRole.push({
    id,
    organizationId: org.id,
    role,
    permission,
    createdAt: new RealDate(start),
    updatedAt: new RealDate(start),
  });
for (let i = 0; i < 105; i++)
  reference.database.organizationRole.push({
    id: "bulk-role-" + i,
    organizationId: org.id,
    role: "bulk-" + i,
    permission: "{}",
    createdAt: new RealDate(start),
  });
reference.database.member.push({
  id: "dangling-role-org-member",
  organizationId: "dangling-org",
  userId: users[3].id,
  role: "viewer",
  createdAt: new RealDate(start),
});
reference.database.organizationRole.push({
  id: "dangling-role-org-grant",
  organizationId: "dangling-org",
  role: "viewer",
  permission: '{"ac":["read","delete"]}',
  createdAt: new RealDate(start),
});
reference.database.organizationRole.find(
  (row) => row.id === "bulk-role-104",
).permission = "{";
reference.database.organizationRole.find(
  (row) => row.id === "bulk-role-103",
).permission = '{"$serde_json::private::Number":"1"}';
for (const [id, role, permission] of [
  ["duplicate-a", "duplicate", "{}"],
  ["duplicate-b", "duplicate", "{}"],
  ["blank-permission", "blank-permission", ""],
  ["null-permission", "null-permission", "null"],
  ["predefined-system", "system", "{}"],
  ["assigned-spaces", "spaced-assigned", "{}"],
  ["substring-role", "substring", "{}"],
  ["cap-role", "cap-role", "{}"],
])
  reference.database.organizationRole.push({
    id,
    role,
    permission,
    organizationId: org.id,
    createdAt: new RealDate(start),
  });
reference.database.member.push({
  id: "spaced-assigned-member",
  organizationId: org.id,
  userId: users[6].id,
  role: "viewer, spaced-assigned ",
  createdAt: new RealDate(start),
});
reference.database.member.push({
  id: "substring-member",
  organizationId: org.id,
  userId: users[6].id,
  role: "substring-more",
  createdAt: new RealDate(start),
});
for (let i = 0; i < 100; i++)
  reference.database.member.push({
    id: "cap-prefix-member-" + i,
    organizationId: org.id,
    userId: users[6].id,
    role: "cap-role-suffix",
    createdAt: new RealDate(start),
  });
reference.database.member.push({
  id: "cap-assigned-member",
  organizationId: org.id,
  userId: users[6].id,
  role: "cap-role",
  createdAt: new RealDate(start),
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
const deleteRole = (body, client = clients[3]) =>
  compare(
    "/organization/delete-role",
    typeof body === "string"
      ? body
      : body === undefined
        ? undefined
        : JSON.stringify(body),
    client,
    "POST",
  );
for (const body of [
  undefined,
  "null",
  "{}",
  '{"roleName":""}',
  '{"roleId":""}',
  '{"roleName":null,"roleId":null}',
  '{"organizationId":null,"roleId":"mixed-role"}',
])
  for (const client of [clients[3], null]) await deleteRole(body, client);
for (const selector of [
  {},
  { roleName: "" },
  { roleId: "" },
  { roleName: "owner" },
  { roleName: "OWNER" },
  { roleName: "viewer" },
  { roleName: "admin" },
  { roleName: "editor" },
  { roleName: "system" },
  { roleName: "MiXeD" },
  { roleName: "mixed" },
  { roleId: "mixed-role" },
  { roleId: "absent" },
  { roleName: "viewer", roleId: "mixed-role" },
  { roleName: "", roleId: "empty-role" },
  { roleName: ["x"], roleId: "spaced-role" },
  { roleName: "duplicate", roleId: ["ignored"] },
  { roleName: "duplicate" },
  { roleId: "bulk-role-104" },
  { roleId: "bulk-role-103" },
  { roleId: "blank-permission" },
  { roleId: "null-permission" },
  { roleId: "predefined-system" },
  { roleId: "assigned-spaces" },
  { roleId: "substring-role" },
  { roleId: "cap-role" },
])
  for (const client of [clients[0], clients[4], clients[5], null, clients[3]])
    await deleteRole(selector, client);
for (const organizationId of [
  "",
  org.id,
  organizations[1].id,
  "absent",
  "dangling-org",
])
  for (const client of [clients[3], clients[4], clients[5]])
    await deleteRole({ organizationId, roleName: "viewer" }, client);
for (const method of ["GET", "HEAD", "OPTIONS"])
  await compare("/organization/delete-role", undefined, clients[3], method);
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
  `Better Auth dynamic role deletion: ${expected.length} actual HTTP/storage/cookie comparisons passed`,
);
