import assert from "node:assert/strict";
import { createHmac, randomUUID } from "node:crypto";
const auth = process.env.AUTH_CHECK_URL ?? "http://auth:3001";
const api = process.env.API_CHECK_URL ?? "http://notes:8000";
const origin = process.env.PUBLIC_ORIGIN;
const workspace = "11111111-1111-4111-8111-111111111111";
const prefix = `/v1/workspaces/${workspace}/notes`;
async function signin(email) {
  const r = await fetch(`${auth}/api/auth/sign-in/email`, {
    method: "POST",
    headers: { "content-type": "application/json", origin },
    body: JSON.stringify({ email, password: process.env.DEMO_PASSWORD }),
  });
  assert.equal(r.status, 200);
  return r.headers
    .getSetCookie()
    .map((v) => v.split(";")[0])
    .join("; ");
}
const alice = await signin("alice@example.test"),
  bob = await signin("bob@example.test");
async function call(
  path,
  { cookie = alice, method = "GET", body, headers = {} } = {},
) {
  return fetch(api + path, {
    method,
    headers: {
      cookie,
      origin,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
      ...headers,
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
}
assert.equal((await call(prefix, { cookie: "" })).status, 401);
assert.equal(
  (
    await call(prefix, {
      cookie: bob,
      method: "POST",
      body: { title: "Denied", body: "Denied" },
    })
  ).status,
  403,
);
assert.equal(
  (
    await call(prefix, {
      method: "POST",
      body: { title: "Denied", body: "Denied" },
      headers: { origin: "https://attacker.invalid" },
    })
  ).status,
  403,
);
const created = await call(prefix, {
  method: "POST",
  body: {
    title: "Acceptance note",
    body: "Fieldnotes confidential acceptance marker. A deterministic worker can summarize these words.",
  },
});
assert.equal(created.status, 201);
const note = await created.json();
assert.equal(note.revision, 1);
const item = `${prefix}/${note.id}`;

// Exercise the mounted internal policy: authenticate original bytes before decoding,
// and consume a valid signed nonce even when the authenticated payload is malformed.
const internalPath = `/internal/workspaces/${workspace}/notes/${note.id}/insight`;
const malformed = '{';
const timestamp = new Date().toISOString();
const nonce = randomUUID();
function signedAuthorization(secret) {
  const signature = createHmac('sha256', secret)
    .update(`POST\n${internalPath}\n${malformed}\n${timestamp}\n${nonce}`)
    .digest('base64');
  return `HMAC keyId=worker ts=${timestamp} nonce=${nonce} signature=${signature}`;
}
async function internalCall(authorization) {
  return fetch(api + internalPath, {
    method: 'POST',
    headers: {'content-type': 'application/json', authorization},
    body: malformed,
  });
}
assert.equal((await internalCall(signedAuthorization('incorrect-secret'))).status, 401,
  'invalid signature fails before malformed JSON decoding and does not claim nonce');
const credential = signedAuthorization(process.env.INTERNAL_AUTH_SECRET);
assert.equal((await internalCall(credential)).status, 400,
  'valid signature reaches typed decoding using the previously unclaimed nonce');
assert.equal((await internalCall(credential)).status, 401,
  'durable nonce rejects replay at the mounted internal route');

for (const body of [
  { expectedRevision: 0, title: "Invalid revision" },
  { expectedRevision: 1, title: null },
  { expectedRevision: 1, title: "x".repeat(513) },
  { expectedRevision: 1, workspaceId: "forged" },
]) {
  assert.equal((await call(item, { method: "PATCH", body })).status, 400,
    "generated request contract rejects invalid data before the handler");
}
assert.equal(
  (
    await call(item, {
      method: "PATCH",
      body: { expectedRevision: 2, title: "Stale" },
    })
  ).status,
  409,
);
const updated = await call(item, {
  method: "PATCH",
  body: { expectedRevision: 1, title: "Acceptance updated" },
});
assert.equal(updated.status, 200);
assert.equal((await updated.json()).revision, 2);
assert.equal(
  (
    await call(
      `/v1/workspaces/33333333-3333-4333-8333-333333333333/notes/${note.id}`,
    )
  ).status,
  403,
);
const exportedText = await call(item + '/export.txt');
assert.equal(exportedText.status, 200);
assert.match(exportedText.headers.get('content-type'), /^text\/plain/);
assert.match(await exportedText.text(), /Acceptance updated/);
const imported = await fetch(api + prefix + '/import', {
  method: 'POST', headers: {cookie: alice, origin, 'content-type': 'application/x-www-form-urlencoded'},
  body: new URLSearchParams({title: '<script>danger()</script>', body: 'HTML escaping fixture'}),
});
assert.equal(imported.status, 201);
const importedNote = await imported.json();
const exportedHtml = await call(prefix + '/' + importedNote.id + '/export.html');
assert.equal(exportedHtml.status, 200);
assert.match(exportedHtml.headers.get('content-type'), /^text\/html/);
assert.match(exportedHtml.headers.get('content-security-policy'), /sandbox/);
const html = await exportedHtml.text();
assert.ok(!html.includes('<script>'));
assert.match(html, /&lt;script&gt;/);
assert.equal((await call(prefix + '/' + importedNote.id, {method:'DELETE'})).status, 204);
const upload = new FormData();
upload.append(
  "file",
  new Blob(["encrypted attachment acceptance marker"], { type: "text/plain" }),
  "review.txt",
);
const uploaded = await fetch(api + item + "/attachments", {
  method: "POST",
  headers: { cookie: alice, origin },
  body: upload,
});
assert.equal(uploaded.status, 201);
const attachment = await uploaded.json();
const download = await call(item + `/attachments/${attachment.id}/content`);
assert.equal(download.status, 200);
assert.equal(await download.text(), "encrypted attachment acceptance marker");
const abort = new AbortController();
const events = await fetch(`${api}/v1/workspaces/${workspace}/events`, {
  headers: { cookie: alice, origin },
  signal: abort.signal,
});
assert.equal(events.status, 200);
assert.match(events.headers.get("content-type"), /text\/event-stream/);
const reader = events.body.getReader();
const queued = await call(item + "/insights", { method: "POST" });
assert.equal(queued.status, 202);
let insight;
for (let i = 0; i < 60; i++) {
  const r = await call(item);
  assert.equal(r.status, 200);
  insight = await r.json();
  if (insight.insightExcerpt) break;
  await new Promise((r) => setTimeout(r, 500));
}
assert.ok(
  insight.insightExcerpt,
  "real worker publishes insight through generated Rust SDK",
);
assert.ok(insight.insightWordCount > 0);
const event = await Promise.race([
  reader.read(),
  new Promise((_, reject) =>
    setTimeout(() => reject(Error("SSE timeout")), 5000),
  ),
]);
assert.ok(event.value?.length);
abort.abort();
const doc = await call("/openapi.json");
assert.equal(doc.status, 200);
const openapi = await doc.json();
assert.ok(openapi.paths[prefix.replace(workspace, "{workspace_id}")]);
assert.ok(
  !Object.keys(openapi.paths).some((path) => path.startsWith("/internal/")),
);
const mcp = await call("/mcp", {
  method: "POST",
  headers: { accept: "application/json, text/event-stream" },
  body: {
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: "2025-03-26",
      capabilities: {},
      clientInfo: { name: "fieldnotes-acceptance", version: "1" },
    },
  },
});
assert.equal(mcp.status, 200);
assert.match(await mcp.text(), /serverInfo/);
// Preserve the primary fixture for ciphertext inspection and manual review.
const temporary = await call(prefix, {method:'POST',body:{title:'Archive fixture',body:'Disposable'}});
assert.equal(temporary.status,201);
const temporaryNote=await temporary.json();
assert.equal((await call(prefix+'/'+temporaryNote.id,{method:'DELETE'})).status,204);
assert.equal((await call(prefix+'/'+temporaryNote.id)).status,404);
console.log(JSON.stringify({retainedNoteId:note.id,attachmentId:attachment.id,workspaceId:workspace}));
console.log(
  "PASS HTTP CRUD, RBAC, CSRF, revision conflict, encrypted attachment roundtrip, real worker insight, SSE, OpenAPI and MCP initialize",
);
