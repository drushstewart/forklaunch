import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const WebSocket = require(process.env.FIELDNOTES_WS_PACKAGE ?? "ws");
const pid = Number(process.env.FIELDNOTES_API_PID);
assert.ok(
  Number.isInteger(pid) && pid > 1,
  "explicit owned native API PID required",
);
const binary = process.env.FIELDNOTES_API_BINARY;
assert.ok(
  binary?.endsWith("/fieldnotes-notes"),
  "explicit native API binary required",
);
assert.equal(
  execFileSync("ps", ["-p", String(pid), "-o", "command="], {
    encoding: "utf8",
  }).trim(),
  binary,
);
const auth = process.env.AUTH_CHECK_URL ?? "http://127.0.0.1:13001";
const api = process.env.API_CHECK_URL ?? "http://127.0.0.1:18000";
const origin = process.env.PUBLIC_ORIGIN;
const signin = await fetch(auth + "/api/auth/sign-in/email", {
  method: "POST",
  headers: { origin, "content-type": "application/json" },
  body: JSON.stringify({
    email: "alice@example.test",
    password: process.env.DEMO_PASSWORD,
  }),
  signal: AbortSignal.timeout(5000),
});
assert.equal(signin.status, 200);
const cookie = signin.headers
  .getSetCookie()
  .map((x) => x.split(";")[0])
  .join("; ");
const path = "/v1/workspaces/11111111-1111-4111-8111-111111111111";
const abort = new AbortController();
let ws;
const timeout = (p, ms) => {
  let timer;
  return Promise.race([
    p,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(Error("shutdown deadline exceeded")), ms);
    }),
  ]).finally(() => clearTimeout(timer));
};
try {
  const response = await timeout(
    fetch(api + path + "/events", {
      headers: { cookie, origin },
      signal: abort.signal,
    }),
    5000,
  );
  assert.equal(response.status, 200);
  const reader = response.body.getReader();
  let sseEnded = false;
  const reading = (async () => {
    while (!(await reader.read()).done) {}
    sseEnded = true;
  })();
  const url = new URL(api + path + "/presence");
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  ws = new WebSocket(url, {
    headers: { cookie, origin },
    handshakeTimeout: 5000,
  });
  await timeout(
    new Promise((resolve, reject) => {
      ws.once("open", resolve);
      ws.once("error", reject);
    }),
    6000,
  );
  const closed = new Promise((resolve) => ws.once("close", resolve));
  process.kill(pid, "SIGTERM");
  await timeout(Promise.all([reading, closed]), 10000);
  assert.ok(sseEnded);
  for (let i = 0; i < 100; i++) {
    try {
      process.kill(pid, 0);
    } catch {
      console.log(
        "PASS SIGTERM closes authenticated active SSE/WebSocket and exits owned native API",
      );
      process.exitCode = 0;
      break;
    }
    await new Promise((r) => setTimeout(r, 100));
    if (i === 99) throw Error("API did not exit");
  }
} finally {
  abort.abort();
  ws?.terminate();
}
