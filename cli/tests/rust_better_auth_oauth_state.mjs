// Actual OAuth state functions, Better Call cookies and memory adapter behavior.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
const actual = (file) =>
  import(pathToFileURL(path.join(dependencies, 'node_modules', file)));
const [
  { betterAuth },
  { createAuthEndpoint },
  { generateState },
  { parseGenericState },
  { z }
] = await Promise.all([
  actual('better-auth/dist/index.mjs'),
  actual('@better-auth/core/dist/api/index.mjs'),
  actual('better-auth/dist/oauth2/state.mjs'),
  actual('better-auth/dist/state.mjs'),
  actual('zod/index.js')
]);
const RealDate = Date,
  start = 1800000000000;
async function atTime(now, work) {
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [now]));
    }
    static [Symbol.hasInstance](value) {
      return value instanceof RealDate;
    }
    static now() {
      return now;
    }
  };
  try {
    return await work();
  } finally {
    globalThis.Date = RealDate;
  }
}
const inputs = [],
  expected = [];
for (const secure of [false, true]) {
  const reference = await betterAuthReference(root, {
    baseURL: secure ? 'https://localhost:3000' : 'http://localhost:3000'
  });
  const plugin = {
    id: 'state-conformance',
    endpoints: {
      generateFixtureState: createAuthEndpoint(
        '/fixture-state-generate',
        { method: 'POST', body: z.record(z.string(), z.any()) },
        async (ctx) => ctx.json(await generateState(ctx))
      ),
      consumeFixtureState: createAuthEndpoint(
        '/fixture-state-consume',
        { method: 'POST', body: z.record(z.string(), z.any()) },
        async (ctx) => {
          try {
            return ctx.json({
              body: await parseGenericState(ctx, ctx.body.state)
            });
          } catch (error) {
            return ctx.json({
              error: {
                code: error.code ?? 'internal_server_error',
                ...(error.errorURL === undefined
                  ? {}
                  : { errorURL: error.errorURL })
              }
            });
          }
        }
      )
    }
  };
  const auth = betterAuth({
    ...reference.config,
    secret: 'better-auth-protocol-fixture-secret-32-characters',
    plugins: [...reference.config.plugins, plugin]
  });
  await auth.$context;
  async function call(route, body, cookie = '', now = start) {
    const response = await atTime(now, () =>
      auth.handler(
        new Request(reference.config.baseURL + '/api/auth/' + route, {
          method: 'POST',
          headers: {
            origin: reference.config.baseURL,
            'content-type': 'application/json',
            cookie
          },
          body: JSON.stringify(body)
        })
      )
    );
    assert.equal(response.status, 200, await response.clone().text());
    return {
      ...(await response.json()),
      cookies: response.headers.getSetCookie()
    };
  }
  const grant = await call('fixture-state-generate', {
    callbackURL: reference.config.baseURL + '/complete',
    errorCallbackURL: reference.config.baseURL + '/failure',
    additionalData: { custom: { value: true } }
  });
  const state = grant.state,
    cookie = grant.cookies[0].split(';')[0];
  assert.equal(state.length, 32);
  assert.equal(grant.codeVerifier.length, 128);
  const initial = JSON.parse(JSON.stringify(reference.database.verification));
  const restore = (rows) =>
    reference.database.verification.splice(
      0,
      reference.database.verification.length,
      ...rows.map((row) => ({
        ...row,
        createdAt: new RealDate(row.createdAt),
        updatedAt: new RealDate(row.updatedAt),
        expiresAt: new RealDate(row.expiresAt)
      }))
    );
  async function compare({
    rows = initial,
    stateValue = state,
    cookieValue = cookie,
    now = start
  } = {}) {
    restore(rows);
    inputs.push({ op: 'seed', now, secure, rows });
    expected.push({ ok: true });
    inputs.push({
      op: 'consume',
      now,
      secure,
      state: stateValue,
      ...(cookieValue === undefined ? {} : { cookie: cookieValue })
    });
    expected.push(
      await call(
        'fixture-state-consume',
        { state: stateValue },
        cookieValue ?? '',
        now
      )
    );
    inputs.push({ op: 'records', now, secure });
    expected.push(JSON.parse(JSON.stringify(reference.database.verification)));
  }
  for (const now of [
    start,
    start + 299999,
    start + 300000,
    start + 600000,
    start + 600001
  ])
    await compare({ now });
  for (const cookieValue of [
    '',
    'better-auth.state=forged',
    cookie + 'forged',
    cookie.replace('state=', 'wrong='),
    cookie + '; ' + cookie
  ])
    await compare({ cookieValue });
  for (const stateValue of ['', 'missing-state', state + 'suffix'])
    await compare({ stateValue });
  for (const patch of [
    { oauthState: 'wrong' },
    { oauthState: null },
    { expiresAt: start - 1 },
    { expiresAt: 'wrong' },
    { callbackURL: null },
    { codeVerifier: null },
    { errorURL: null },
    { link: { email: 'fixture@test', userId: 42, strip: true } },
    { requestSignUp: null },
    { serverContext: null }
  ]) {
    const rows = structuredClone(initial);
    rows[0].value = JSON.stringify({ ...JSON.parse(rows[0].value), ...patch });
    await compare({ rows });
  }
  for (const field of [
    'oauthState',
    'callbackURL',
    'codeVerifier',
    'expiresAt'
  ]) {
    const rows = structuredClone(initial),
      value = JSON.parse(rows[0].value);
    delete value[field];
    rows[0].value = JSON.stringify(value);
    await compare({ rows });
  }
  for (const value of ['not-json', '{}', 'null', '[]']) {
    const rows = structuredClone(initial);
    rows[0].value = value;
    await compare({ rows });
  }
  await compare({ rows: [] });
  const rows = structuredClone(initial);
  rows.push({
    ...rows[0],
    id: 'expired-unrelated',
    identifier: 'unrelated',
    expiresAt: new RealDate(start - 1).toISOString()
  });
  await compare({ rows, stateValue: 'missing-state' });
}
const run = spawnSync(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    'blueprint-rs/Cargo.toml',
    '-p',
    'forklaunch-iam-better-auth',
    '--example',
    'oauth_state_parity'
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    input: inputs.map((value) => JSON.stringify(value)).join('\n') + '\n',
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024
  }
);
assert.equal(run.status, 0, run.stderr);
const results = run.stdout
  .trim()
  .split('\n')
  .map((line) => JSON.parse(line));
assert.equal(results.length, expected.length);
const differences = [];
for (let i = 0; i < results.length; i++)
  try {
    assert.deepEqual(results[i], expected[i]);
  } catch (error) {
    differences.push({ case: i, input: inputs[i], diff: error.message });
  }
assert.deepEqual(differences.slice(0, 10), []);
console.log(
  `Better Auth OAuth database state: ${inputs.filter((input) => input.op === 'consume').length} actual state/cookie and ${inputs.filter((input) => input.op === 'records').length} storage comparisons passed`
);
