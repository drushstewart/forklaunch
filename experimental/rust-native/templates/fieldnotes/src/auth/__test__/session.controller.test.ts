import assert from "node:assert/strict";
import { once } from "node:events";
import type { AddressInfo } from "node:net";
import { test } from "node:test";
import { createServer } from "../server.js";
import type { AuthDependencies } from "../registrations.js";

const workspaceId = "AAAAAAAA-1111-4111-8111-111111111111";
const internalSecret = "a".repeat(32);

test("session routes retain private authorization and public cookie refresh behavior", async () => {
  const calls: {
    query: { disableCookieCache: boolean; disableRefresh?: boolean };
  }[] = [];
  const dependencies = {
    config: { port: 0, internalSecret },
    auth: {
      handler: () => new Response(null, { status: 404 }),
      api: {
        getSession: async (options: {
          query: { disableCookieCache: boolean; disableRefresh?: boolean };
          returnHeaders?: boolean;
        }) => {
          calls.push(options);
          const session = { user: { id: "alice" } };
          return options.returnHeaders
            ? {
                response: session,
                headers: new Headers({
                  "Set-Cookie": "session=refreshed; Secure; HttpOnly",
                }),
              }
            : session;
        },
      },
    },
    repository: {
      membership: async (workspace: string, user: string) => {
        assert.equal(workspace, workspaceId);
        assert.equal(user, "alice");
        return "admin";
      },
      listWorkspaces: async () => [
        { id: workspaceId, name: "Shared", role: "admin" },
      ],
    },
  } as unknown as AuthDependencies;
  const server = createServer(dependencies);
  await once(server, "listening");
  const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  try {
    const unauthorized = await fetch(`${base}/internal/session`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ workspaceId }),
    });
    assert.equal(unauthorized.status, 401);
    assert.equal(calls.length, 0);
    const invalid = await fetch(`${base}/internal/session`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-fieldnotes-internal": internalSecret,
      },
      body: JSON.stringify({ workspaceId: "invalid" }),
    });
    assert.equal(invalid.status, 400);
    assert.equal(calls.length, 0);
    const authorized = await fetch(`${base}/internal/session`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-fieldnotes-internal": internalSecret,
      },
      body: JSON.stringify({ workspaceId }),
    });
    assert.equal(authorized.status, 200);
    assert.equal(authorized.headers.get("Cache-Control"), "no-store");
    assert.equal(authorized.headers.get("Set-Cookie"), null);
    assert.deepEqual(await authorized.json(), {
      userId: "alice",
      workspaceId: workspaceId.toLowerCase(),
      role: "admin",
    });
    assert.deepEqual(calls[0]?.query, {
      disableCookieCache: true,
      disableRefresh: true,
    });
    const publicResponse = await fetch(`${base}/api/workspaces`);
    assert.equal(publicResponse.status, 200);
    assert.equal(
      publicResponse.headers.get("Set-Cookie"),
      "session=refreshed; Secure; HttpOnly",
    );
    assert.equal(publicResponse.headers.get("Cache-Control"), "no-store");
    assert.deepEqual(calls[1]?.query, { disableCookieCache: true });
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
