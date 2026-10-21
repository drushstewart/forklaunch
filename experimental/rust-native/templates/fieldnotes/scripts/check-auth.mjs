import assert from "node:assert/strict";
const base = "http://localhost:3001";
const headers = {
  "content-type": "application/json",
  origin: process.env.PUBLIC_ORIGIN,
};
const signin = await fetch(`${base}/api/auth/sign-in/email`, {
  method: "POST",
  headers,
  body: JSON.stringify({
    email: "alice@example.test",
    password: process.env.DEMO_PASSWORD,
  }),
});
assert.equal(signin.status, 200, "real Better Auth sign-in");
const cookie = signin.headers
  .getSetCookie()
  .map((value) => value.split(";")[0])
  .join("; ");
assert.ok(cookie, "session cookie");
assert.ok(
  signin.headers.getSetCookie().some((value) => value.includes("Secure")),
  "HTTPS secure cookie",
);
const workspaces = await fetch(`${base}/api/workspaces`, {
  headers: { cookie },
});
assert.equal(workspaces.status, 200);
const data = await workspaces.json();
assert.equal(data.workspaces.length, 2);
assert.ok(
  data.workspaces.every(
    (value) => value.id !== "33333333-3333-4333-8333-333333333333",
  ),
);
for (const [workspaceId, expected] of [
  ["11111111-1111-4111-8111-111111111111", 200],
  ["33333333-3333-4333-8333-333333333333", 403],
]) {
  const result = await fetch(`${base}/internal/session`, {
    method: "POST",
    headers: {
      ...headers,
      cookie,
      "x-fieldnotes-internal": process.env.INTERNAL_AUTH_SECRET,
    },
    body: JSON.stringify({ workspaceId }),
  });
  assert.equal(result.status, expected, "durable membership authorization");
}
assert.equal((await fetch(`${base}/api/workspaces`)).status, 401);
const forged = await fetch(`${base}/internal/session`, {
  method: "POST",
  headers: { ...headers, cookie, "x-fieldnotes-internal": "forged" },
  body: JSON.stringify({ workspaceId: "11111111-1111-4111-8111-111111111111" }),
});
assert.equal(forged.status, 401);
console.log(
  "PASS real sign-in, secure cookie, membership isolation, anonymous and forged-secret rejection",
);
