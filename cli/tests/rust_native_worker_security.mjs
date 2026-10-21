// Run inside a generated app's auth container (which owns INTERNAL_AUTH_SECRET):
// docker compose exec -T auth node --input-type=module < PATH/TO/THIS/FILE
import assert from 'node:assert/strict';
import { createHmac, randomUUID } from 'node:crypto';

const secret = process.env.INTERNAL_AUTH_SECRET;
assert.ok(secret?.length >= 32, 'An internal signing key is required');
const base = process.env.WORKER_SECURITY_API_URL ?? 'http://notes:8000';
const path = `/internal/workspaces/${randomUUID()}/notes/${randomUUID()}/insight`;
const body = JSON.stringify({ operationId: randomUUID(), revision: 1, encryptedPayload: 'invalid-envelope' });
function authorization(body, nonce = randomUUID(), date = new Date()) {
  const ts = date.toISOString();
  const signature = createHmac('sha256', secret)
    .update(`POST\n${path}\n${body}\n${ts}\n${nonce}`).digest('base64');
  return `HMAC keyId=worker ts=${ts} nonce=${nonce} signature=${signature}`;
}
async function request(header, payload = body, extra = {}) {
  const response = await fetch(base + path, {
    method: 'POST', headers: { 'content-type': 'application/json', authorization: header, ...extra }, body: payload,
  });
  await response.arrayBuffer();
  return response.status;
}

// A valid signature reaches envelope validation. Reuse must stop before decode,
// backed by the shared PostgreSQL nonce table, not an in-process cache.
const header = authorization(body);
assert.equal(await request(header), 400);
assert.equal(await request(header), 401);
const concurrent = authorization(body);
const concurrentStatuses = await Promise.all(Array.from({length: 8}, () => request(concurrent)));
assert.equal(concurrentStatuses.filter(status => status === 400).length, 1, 'Exactly one concurrent nonce claim may reach envelope validation');
assert.equal(concurrentStatuses.filter(status => status === 401).length, 7);
const altered = authorization(body);
assert.equal(await request(altered, body + ' '), 401);
assert.equal(await request(altered), 400, 'Bad signature must not consume the valid nonce');
assert.equal(await request(authorization(body, randomUUID(), new Date(Date.now() - 120_000))), 401);
assert.equal(await request(authorization(body, randomUUID(), new Date(Date.now() + 120_000))), 401);
assert.equal(await request('', body, { 'x-fieldnotes-internal': secret }), 401,
  'Legacy bearer-style callback authentication must not be accepted');
console.log('PASS worker callback HMAC, durable nonce replay rejection, freshness, body binding and legacy credential rejection');
