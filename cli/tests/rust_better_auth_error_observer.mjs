// Execute the actual configured unexpected-error hook and safe diagnostic helper.
// Only the adapter failure and telemetry sink are owned by this fixture.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
import { actualValidator } from './rust_actual_validator_fixture.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const { betterAuth } = await import(pathToFileURL(path.join(dependencies, 'node_modules/better-auth/dist/index.mjs')));
const { APIError } = await import(pathToFileURL(path.join(dependencies, 'node_modules/better-auth/dist/api/index.mjs')));
const reference = await betterAuthReference(root);
const { load } = actualValidator(root, () => undefined, { sameRealm: true });
const { logBetterAuthApiError } = load(path.join(root, 'blueprint/iam-better-auth/domain/utils/betterAuthErrorLogging.util.ts'));
let failing = false;
const observations = [];
const diagnostics = [];
const auth = betterAuth({
  ...reference.config,
  secret: 'better-auth-protocol-fixture-secret-32-characters',
  database: options => ({
    ...reference.config.database(options),
    findOne: async args => {
      if (failing === 'handled') throw new APIError('INTERNAL_SERVER_ERROR', { message: 'Owned handled server error' });
      if (failing) throw Object.assign(new Error('Owned synthetic database failure'), {
        code: '42703', column: 'owned_fixture_column', detail: 'PRIVATE_ROW', query: 'PRIVATE_QUERY', where: 'PRIVATE_WHERE',
      });
      return reference.config.database(options).findOne(args);
    },
  }),
  onAPIError: { onError(error, context) {
    observations.push(error);
    logBetterAuthApiError({ error: (...args) => diagnostics.push(args) }, error, context);
  } },
});
async function request(route, body, method) {
  return auth.handler(new Request(`http://localhost:3000/api/auth${route}`, {
    method: method ?? (body ? 'POST' : 'GET'),
    headers: { 'content-type': 'application/json', origin: 'http://localhost:3000' },
    ...(body ? { body: JSON.stringify(body) } : {}),
  }));
}
const credentials = { email: 'owned@example.test', password: 'owned-fixture-password' };
for (const [route, body, method, status] of [
  ['/get-session', undefined, 'GET', 200],
  ['/token', undefined, 'GET', 401],
  ['/sign-in/email', credentials, 'POST', 401],
  ['/sign-up/email', { ...credentials, password: 'short' }, 'POST', 400],
  ['/get-session', undefined, 'POST', 405],
  ['/missing', undefined, 'GET', 404],
]) {
  assert.equal((await request(route, body, method)).status, status, route);
  assert.equal(observations.length, 0, `${route} must not invoke unexpected-error hook`);
}
failing = true;
const response = await request('/sign-in/email', credentials);
assert.equal(response.status, 500);
assert.equal(await response.text(), '');
assert.equal(observations.length, 1);
assert.equal(diagnostics.length, 1);
assert.equal(diagnostics[0][1].code, '42703');
assert.equal(diagnostics[0][1].column, 'owned_fixture_column');
assert.ok(!JSON.stringify(diagnostics).includes('PRIVATE_'));
// The native observer deliberately covers all raw5xx responses: the upstream
// hook excludes handled APIError500 as well as ordinary authentication denials.
failing = 'handled';
assert.equal((await request('/sign-in/email', credentials)).status, 500);
assert.equal(observations.length, 1);
const leaf = Object.assign(new Error('Owned structural failure'), { code: '23502', constraint: 'owned_constraint', table: 'owned_table', detail: 'PRIVATE_ROW' });
const outer = Object.assign(new Error('Owned wrapper'), { cause: leaf, code: 'OUTER_CODE' });
leaf.cause = outer;
logBetterAuthApiError({ error: (...args) => diagnostics.push(args) }, outer, { context: { path: '/sign-in/email' } });
const metadata = diagnostics.at(-1)[1];
assert.equal(metadata.code, 'OUTER_CODE');
assert.equal(metadata.table, 'owned_table');
assert.equal(metadata.constraint, 'owned_constraint');
assert.equal(metadata.path, '/sign-in/email');
assert.ok(metadata.stack.length <= 2000);
assert.ok(!JSON.stringify(metadata).includes('PRIVATE_'));
// The actual helper isolates telemetry failure. Capture its static fallback log.
const fallback = [];
const saved = console.error;
try {
  console.error = (...args) => fallback.push(args);
  assert.doesNotThrow(() => logBetterAuthApiError({ error() { throw new Error('Owned telemetry failure'); } }, outer));
} finally { console.error = saved; }
assert.equal(fallback.length, 1);
assert.equal(fallback[0][0], '[IAM] Failed to log Better Auth API error');
console.log('Better Auth configured error observer: six expected outcomes and handled APIError500 skipped, one unexpected storage failure observed, structural sanitization/cyclic causes and telemetry failure isolation passed.');
