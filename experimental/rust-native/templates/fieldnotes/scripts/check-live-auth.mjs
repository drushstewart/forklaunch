import assert from "node:assert/strict";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { Pool } = require(
  process.env.FIELDNOTES_PG_PACKAGE ?? "../src/auth/node_modules/pg",
);
const WebSocket = require(process.env.FIELDNOTES_WS_PACKAGE ?? "ws");
const auth = process.env.AUTH_CHECK_URL ?? "http://auth:3001";
const api = process.env.API_CHECK_URL ?? "http://notes:8000";
const origin = process.env.PUBLIC_ORIGIN;
const shared = "11111111-1111-4111-8111-111111111111";
const privateAlice = "22222222-2222-4222-8222-222222222222";
const pool = new Pool({
  connectionString: process.env.DATABASE_URL,
  connectionTimeoutMillis: 5000,
  query_timeout: 5000,
});
const sockets = [];
const created = [];
const abort = new AbortController();
let original;
let alice;
function deadline(promise, ms, label) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(Error(`${label} timeout`)), ms);
    }),
  ]).finally(() => clearTimeout(timer));
}
const delay = (ms) => new Promise((r) => setTimeout(r, ms));
async function signin(email) {
  const r = await fetch(`${auth}/api/auth/sign-in/email`, {
    method: "POST",
    headers: { "content-type": "application/json", origin },
    body: JSON.stringify({ email, password: process.env.DEMO_PASSWORD }),
    signal: AbortSignal.timeout(5000),
  });
  assert.equal(r.status, 200);
  return r.headers
    .getSetCookie()
    .map((v) => v.split(";")[0])
    .join("; ");
}
async function call(
  workspace,
  method = "GET",
  body,
  cookie = alice,
  suffix = "",
) {
  return fetch(`${api}/v1/workspaces/${workspace}/notes${suffix}`, {
    method,
    headers: {
      cookie,
      origin,
      ...(body === undefined ? {} : { "content-type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.timeout(10000),
  });
}
async function socket(
  workspace,
  cookie,
  expected = 101,
  requestOrigin = origin,
) {
  const target = new URL(`/v1/workspaces/${workspace}/presence`, api);
  target.protocol = target.protocol === "https:" ? "wss:" : "ws:";
  const ws = new WebSocket(target, {
    headers: { cookie, origin: requestOrigin },
    handshakeTimeout: 5000,
  });
  sockets.push(ws);
  return deadline(
    new Promise((resolve, reject) => {
      ws.once("open", () => {
        try {
          assert.equal(expected, 101);
          resolve(ws);
        } catch (e) {
          reject(e);
        }
      });
      ws.once("unexpected-response", (_request, response) => {
        response.resume();
        try {
          assert.equal(response.statusCode, expected);
          ws.terminate();
          resolve(null);
        } catch (e) {
          reject(e);
        }
      });
      ws.on("error", (error) => {
        if (expected === 101) reject(error);
      });
    }),
    6000,
    "WebSocket handshake",
  );
}
function messages(ws) {
  const queue = [];
  ws.on("message", (data) => queue.push(JSON.parse(data.toString())));
  return queue;
}
async function until(predicate, ms, label) {
  const end = Date.now() + ms;
  while (!predicate()) {
    if (Date.now() > end) throw Error(`${label} timeout`);
    await delay(30);
  }
}
try {
  alice = await signin("alice@example.test");
  const bob = await signin("bob@example.test");
  const bobId = (
    await pool.query('SELECT id FROM auth."user" WHERE email=$1', [
      "bob@example.test",
    ])
  ).rows[0].id;
  await socket(shared, "", 401);
  await socket(privateAlice, bob, 403);
  await socket(shared, alice, 403, "https://attacker.invalid");
  const bobSocket = await socket(shared, bob);
  const aliceSocket = await socket(shared, alice);
  const privateSocket = await socket(privateAlice, alice);
  const bobMessages = messages(bobSocket);
  const aliceMessages = messages(aliceSocket);
  const privateMessages = messages(privateSocket);
  const response = await deadline(
    fetch(`${api}/v1/workspaces/${shared}/events`, {
      headers: { cookie: bob, origin },
      signal: abort.signal,
    }),
    6000,
    "SSE handshake",
  );
  assert.equal(response.status, 200);
  const events = [];
  let streamDone = false;
  let streamError;
  const reader = response.body.getReader();
  const readLoop = (async () => {
    let text = "";
    const decoder = new TextDecoder();
    try {
      while (true) {
        const next = await reader.read();
        if (next.done) {
          streamDone = true;
          break;
        }
        text += decoder.decode(next.value, { stream: true });
        let at;
        while ((at = text.indexOf("\n\n")) >= 0) {
          const frame = text.slice(0, at);
          text = text.slice(at + 2);
          const data = frame
            .split("\n")
            .filter((l) => l.startsWith("data:"))
            .map((l) => l.slice(5).trimStart())
            .join("\n");
          if (data) events.push(JSON.parse(data));
        }
      }
    } catch (e) {
      streamError = e;
    }
  })();
  const sharedResponse = await call(shared, "POST", {
    title: "Live authorization proof",
    body: "Synthetic shared event.",
  });
  assert.equal(sharedResponse.status, 201);
  const sharedNote = await sharedResponse.json();
  created.push([shared, sharedNote.id]);
  const privateResponse = await call(privateAlice, "POST", {
    title: "Private live proof",
    body: "Synthetic private event.",
  });
  assert.equal(privateResponse.status, 201);
  const privateNote = await privateResponse.json();
  created.push([privateAlice, privateNote.id]);
  await until(
    () => events.some((e) => e.noteId === sharedNote.id),
    5000,
    "shared SSE event",
  );
  aliceSocket.send(JSON.stringify({ noteId: sharedNote.id }));
  await until(
    () => bobMessages.some((e) => e.noteId === sharedNote.id),
    3000,
    "shared presence",
  );
  bobSocket.send(JSON.stringify({ noteId: sharedNote.id }));
  await until(
    () =>
      aliceMessages.some(
        (e) => e.noteId === sharedNote.id && e.userId === bobId,
      ),
    3000,
    "viewer presence",
  );
  privateSocket.send(JSON.stringify({ noteId: privateNote.id }));
  await until(
    () => privateMessages.some((e) => e.noteId === privateNote.id),
    3000,
    "private presence",
  );
  await delay(1500);
  assert.ok(
    !bobMessages.some((e) => e.noteId === privateNote.id),
    "presence cannot cross workspace",
  );
  assert.ok(
    !events.some((e) => e.noteId === privateNote.id),
    "SSE cannot cross workspace",
  );
  original = (
    await pool.query(
      "DELETE FROM auth.memberships WHERE workspace_id=$1 AND user_id=$2 RETURNING *",
      [shared, bobId],
    )
  ).rows[0];
  assert.ok(original);
  let wsClosed = bobSocket.readyState === WebSocket.CLOSED;
  bobSocket.once("close", () => {
    wsClosed = true;
  });
  await until(
    () => streamDone || streamError,
    8000,
    "SSE membership revocation",
  );
  assert.equal(streamError, undefined, "SSE must close cleanly");
  await until(() => wsClosed, 12000, "WebSocket membership revocation");
  assert.equal(
    (
      await fetch(`${api}/v1/workspaces/${shared}/events`, {
        headers: { cookie: bob, origin },
        signal: AbortSignal.timeout(5000),
      })
    ).status,
    403,
  );
  await socket(shared, bob, 403);
  await readLoop;
} finally {
  abort.abort();
  for (const ws of sockets) ws.terminate();
  if (original)
    await pool.query(
      "INSERT INTO auth.memberships(workspace_id,user_id,role) VALUES($1,$2,$3) ON CONFLICT(workspace_id,user_id) DO UPDATE SET role=EXCLUDED.role",
      [original.workspace_id, original.user_id, original.role],
    );
  if (alice)
    for (const [workspace, id] of created)
      assert.equal(
        (await call(workspace, "DELETE", undefined, alice, `/${id}`)).status,
        204,
        "owned note cleanup",
      );
  await pool.end();
}

console.log(
  "PASS authenticated viewer presence, WebSocket/SSE tenant isolation, anonymous/foreign-origin rejection, active membership revocation and owned note cleanup",
);
