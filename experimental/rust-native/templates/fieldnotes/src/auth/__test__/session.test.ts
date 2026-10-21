import assert from "node:assert/strict";
import { test } from "node:test";
import { parseSessionRequest } from "../domain/schemas/session.schema.js";

test("internal session rejects missing, malformed and non-string tenant IDs", () => {
  for (const value of [
    null,
    undefined,
    [],
    {},
    { workspaceId: 1 },
    { workspaceId: "not-a-uuid" },
  ])
    assert.equal(parseSessionRequest(value), undefined);
});
test("internal session accepts a UUID and preserves case for the repository lookup", () => {
  const workspaceId = "AAAAAAAA-1111-4111-8111-111111111111";
  assert.deepEqual(parseSessionRequest({ workspaceId }), { workspaceId });
});
