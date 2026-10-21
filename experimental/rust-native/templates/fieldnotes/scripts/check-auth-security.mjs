import assert from "node:assert/strict";
import pg from "pg";
const base = "http://localhost:3001";
const pool = new pg.Pool({
  connectionString: process.env.DATABASE_URL,
  options: "-c search_path=auth,public",
});
const headers = {
  "content-type": "application/json",
  origin: process.env.PUBLIC_ORIGIN,
};
const shared = "11111111-1111-4111-8111-111111111111";
const alicePrivate = "22222222-2222-4222-8222-222222222222";
const bobPrivate = "33333333-3333-4333-8333-333333333333";
let original;
try {
  const signIn = await fetch(`${base}/api/auth/sign-in/email`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      email: "bob@example.test",
      password: process.env.DEMO_PASSWORD,
    }),
  });
  assert.equal(signIn.status, 200);
  const cookie = signIn.headers
    .getSetCookie()
    .map((value) => value.split(";")[0])
    .join("; ");
  const internal = async (workspaceId, extra = {}, credentials = cookie) =>
    fetch(`${base}/internal/session`, {
      method: "POST",
      headers: {
        ...headers,
        cookie: credentials,
        "x-fieldnotes-internal": process.env.INTERNAL_AUTH_SECRET,
        ...extra,
      },
      body: JSON.stringify({ workspaceId }),
    });
  const viewer = await internal(shared);
  assert.equal(viewer.status, 200);
  assert.equal((await viewer.json()).role, "viewer");
  const own = await internal(bobPrivate);
  assert.equal(own.status, 200);
  assert.equal((await own.json()).role, "admin");
  assert.equal(
    (
      await internal(alicePrivate, {
        "x-user-id": "alice",
        "x-tenant-id": alicePrivate,
      })
    ).status,
    403,
  );
  assert.equal(
    (
      await internal(
        shared,
        { "x-user-id": "alice", "x-tenant-id": shared },
        "",
      )
    ).status,
    401,
  );
  const spaces = await (
    await fetch(`${base}/api/workspaces`, {
      headers: { cookie, "x-user-id": "alice" },
    })
  ).json();
  assert.deepEqual(
    new Set(spaces.workspaces.map((w) => w.id)),
    new Set([shared, bobPrivate]),
  );
  const bob = await pool.query('SELECT id FROM auth."user" WHERE email=$1', [
    "bob@example.test",
  ]);
  original = (
    await pool.query(
      "DELETE FROM auth.memberships WHERE workspace_id=$1 AND user_id=$2 RETURNING *",
      [shared, bob.rows[0].id],
    )
  ).rows[0];
  assert.ok(original);
  assert.equal(
    (await internal(shared)).status,
    403,
    "membership revocation effective immediately",
  );
  await pool.query(
    "INSERT INTO auth.memberships(workspace_id,user_id,role) VALUES($1,$2,$3)",
    [original.workspace_id, original.user_id, original.role],
  );
  original = undefined;
  assert.equal(
    (await internal(shared)).status,
    200,
    "restored membership visible",
  );
  const denied = await fetch(`${base}/api/auth/sign-out`, {
    method: "POST",
    headers: { ...headers, origin: "https://attacker.invalid", cookie },
    body: "{}",
  });
  assert.equal(denied.status, 403, "cross-origin sign-out denied");
  assert.equal(
    (await internal(shared)).status,
    200,
    "denied CSRF did not revoke session",
  );
  const sessionReply = await fetch(`${base}/api/auth/get-session`, {
    headers: { cookie },
  });
  assert.equal(sessionReply.status, 200);
  const current = await sessionReply.json();
  assert.ok(current.session.id);
  await pool.query('DELETE FROM auth.session WHERE id=$1 AND "userId"=$2', [
    current.session.id,
    bob.rows[0].id,
  ]);
  assert.equal(
    (await internal(shared)).status,
    401,
    "durable session revocation effective immediately",
  );
  console.log(
    "PASS auth security assertions: roles, tenant isolation, forged identity, membership/session revocation, CSRF; membership restored",
  );
} finally {
  if (original)
    await pool.query(
      "INSERT INTO auth.memberships(workspace_id,user_id,role) VALUES($1,$2,$3) ON CONFLICT(workspace_id,user_id) DO UPDATE SET role=EXCLUDED.role",
      [original.workspace_id, original.user_id, original.role],
    );
  await pool.end();
}
