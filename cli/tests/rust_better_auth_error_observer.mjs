// Execute the actual configured unexpected-error hook and safe diagnostic helper.
// Only the adapter failure and telemetry sink are owned by this fixture.
import assert from 'node:assert/strict';
import path from 'node:path';
import {readFileSync} from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
import { actualValidator } from './rust_actual_validator_fixture.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
assert.match(readFileSync(path.join(root,'cli/src/templates/rust/iam-better-auth/main.rs'),'utf8'), /with_api_error_observer_mode\(AuthApiErrorObserverMode::UnexpectedOnly\)/);
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
      if (failing === true) throw Object.assign(new Error('Owned synthetic database failure'), {
        code: '42703', column: 'owned_fixture_column', detail: 'PRIVATE_ROW', query: 'PRIVATE_QUERY', where: 'PRIVATE_WHERE',
      });
      return reference.config.database(options).findOne(args);
    },
    delete: async args => {
      if(failing==='delete')throw new Error('Owned delete failure');
      return reference.config.database(options).delete(args);
    },
    deleteMany: async args => {
      if(failing==='delete')throw new Error('Owned delete failure');
      return reference.config.database(options).deleteMany(args);
    },
  }),
  onAPIError: { onError(error, context) {
    observations.push(error);
    logBetterAuthApiError({ error: (...args) => diagnostics.push(args) }, error, context);
  } },
});
let cookie = '';
async function request(route, body, method) {
  return auth.handler(new Request(`http://localhost:3000/api/auth${route}`, {
    method: method ?? (body ? 'POST' : 'GET'),
    headers: { 'content-type': 'application/json', origin: 'http://localhost:3000', ...(cookie ? {cookie} : {}) },
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
// The explicit unexpected-only mode excludes handled APIError500. The native
// compatibility observer continues covering every raw5xx when selected.
failing = 'handled';
assert.equal((await request('/sign-in/email', credentials)).status, 500);
assert.equal(observations.length, 1);
// Real configured session and dynamic-permission boundaries preserve whether
// the upstream implementation catches/rethrows a handled APIError.
failing = false;
const signup = await request('/sign-up/email', {...credentials, name:'Owned Observer',firstName:'Owned',lastName:'Observer'});
assert.equal(signup.status,200);
cookie = signup.headers.getSetCookie().find(value=>value.startsWith('better-auth.session_token=')).split(';')[0];
failing = true;
const beforeSession = observations.length;
assert.equal((await request('/get-session')).status,500);
assert.equal(observations.length,beforeSession,'getSession converts adapter failures to handled APIError');
failing = false;
const organizationResponse = await request('/organization/create',{name:'Observer Org',slug:'observer-org'});
assert.equal(organizationResponse.status,200);
const organization = await organizationResponse.json();
const context = await auth.$context;
const role = await context.adapter.create({model:'organizationRole',data:{organizationId:organization.id,role:'broken-role',permission:'null',createdAt:new Date(),updatedAt:new Date()}});
const beforePermission = observations.length;
const permissionBody = {organizationId:organization.id,permissions:{platform:['platform:read']}};
assert.equal((await request('/organization/has-permission',permissionBody)).status,500);
assert.equal(observations.length,beforePermission,'parsed invalid permission schema is a handled APIError');
await context.adapter.update({model:'organizationRole',where:[{field:'id',value:role.id}],update:{permission:'{'}});
assert.equal((await request('/organization/has-permission',permissionBody)).status,500);
assert.equal(observations.length,beforePermission+1,'JSON syntax failure remains unexpected');
const signin = await request('/sign-in/email',credentials);
assert.equal(signin.status,200);
const secondCookie=signin.headers.getSetCookie().find(value=>value.startsWith('better-auth.session_token=')).split(';')[0];
const firstCookie=cookie;
cookie=secondCookie;
const secondSession=await (await request('/get-session')).json();
cookie=firstCookie;
failing='delete';
for(const [route,body,increment] of [
 ['/revoke-session',{token:secondSession.session.token},0],
 ['/revoke-sessions',{},0],
 ['/revoke-other-sessions',{},1],
]) {
 const before=observations.length;
 assert.equal((await request(route,body)).status,500,route);
 assert.equal(observations.length,before+increment,route);
}
failing=false;
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
console.log('Better Auth configured error observer: session/permission/revocation provenance verified; six expected outcomes and handled APIError500 skipped, one unexpected storage failure observed, structural sanitization/cyclic causes and telemetry failure isolation passed.');
