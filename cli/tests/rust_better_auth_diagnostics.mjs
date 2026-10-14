import assert from 'node:assert/strict';
import { pathToFileURL, fileURLToPath } from 'node:url';
import path from 'node:path';
import { readFileSync } from 'node:fs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
assert.match(
  readFileSync(
    path.join(root, 'cli/src/templates/rust/iam-better-auth/main.rs'),
    'utf8'
  ),
  /with_api_error_trace_mode\(AuthApiErrorTraceMode::Configured\)/
);
const { betterAuthReference } = await import(
  pathToFileURL(root + '/cli/tests/fixtures/rust_better_auth_reference.mjs')
);
const prefix =
  process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES + '/node_modules/';
const { betterAuth } = await import(
  pathToFileURL(prefix + 'better-auth/dist/index.mjs')
);
const { APIError } = await import(
  pathToFileURL(prefix + 'better-auth/dist/api/index.mjs')
);
const reference = await betterAuthReference(root);
let failure = '',
  handled = false,
  cookie = '';
const logs = [],
  observations = [],
  calls = [];
const wrap = (a) => {
  const b = { ...a };
  for (const method of [
    'findOne',
    'findMany',
    'create',
    'update',
    'updateMany',
    'delete',
    'deleteMany',
    'count'
  ])
    if (a[method])
      b[method] = async (args) => {
        calls.push(`${method}:${args.model}`);
        if (failure === `${method}:${args.model}`)
          throw handled
            ? new APIError('INTERNAL_SERVER_ERROR', {
                message: 'Owned handled'
              })
            : new Error('Owned unexpected');
        return a[method](args);
      };
  return b;
};
const auth = betterAuth({
  ...reference.config,
  database: (options) => wrap(reference.config.database(options)),
  logger: { level: 'error', log: (...args) => logs.push(args) },
  onAPIError: { onError: (error) => observations.push(error) }
});
async function request(route, body) {
  const r = await auth.handler(
    new Request('http://localhost:3000/api/auth' + route, {
      method: body ? 'POST' : 'GET',
      headers: {
        origin: 'http://localhost:3000',
        'content-type': 'application/json',
        cookie
      },
      ...(body ? { body: JSON.stringify(body) } : {})
    })
  );
  return {
    status: r.status,
    body: await r.text(),
    cookies: r.headers.getSetCookie()
  };
}
const signup = await request('/sign-up/email', {
  email: 'diagnostic@example.test',
  password: 'owned-fixture-password',
  name: 'Owned',
  firstName: 'Owned',
  lastName: 'Audit'
});
assert.equal(signup.status, 200);
cookie = signup.cookies
  .find((c) => c.startsWith('better-auth.session_token='))
  .split(';')[0];
const org = JSON.parse(
  (await request('/organization/create', { name: 'Audit', slug: 'diag-audit' }))
    .body
);
let checked = 0;
const savedConsoleError = console.error;
console.error = () => {}; // Better Call's freeform fallback is intentionally not replayed.
try {
  for (const [route, body, fault] of [
    ['/list-sessions', undefined, 'findMany:session'],
    ['/update-user', { name: 'Changed' }, 'update:user'],
    ['/organization/list', undefined, 'findMany:member'],
    [
      '/organization/list-members?organizationId=' + org.id,
      undefined,
      'findMany:member'
    ],
    ['/token', undefined, 'findMany:jwks'],
    ['/jwks', undefined, 'findMany:jwks'],
    ['/get-session', undefined, 'findOne:session']
  ])
    for (handled of [false, true]) {
      failure = fault;
      logs.length = 0;
      observations.length = 0;
      calls.length = 0;
      const result = await request(route, body);
      assert.equal(result.status, 500);
      assert.ok(calls.includes(fault));
      const caught = route === '/list-sessions' || route === '/get-session';
      assert.equal(observations.length, handled || caught ? 0 : 1, route);
      assert.equal(
        logs.length,
        route === '/list-sessions' || (route === '/get-session' && !handled)
          ? 1
          : 0,
        route
      );
      if (route === '/list-sessions' || (!handled && !caught))
        assert.equal(result.body, '');
      checked++;
    }
} finally {
  console.error = savedConsoleError;
}
assert.equal(checked, 14);
console.log(
  'PASS configured diagnostic catch matrix: 14 actual Better Auth cases across session listing/read, user update, organization lists, token and JWKS; callback vs internal logger counts asserted. Freeform fallback logs excluded.'
);
