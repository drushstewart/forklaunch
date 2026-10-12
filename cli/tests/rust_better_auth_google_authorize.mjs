// Actual configured Google provider URL construction, PKCE and parameter ordering.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const { google } = await import(
  pathToFileURL(
    path.join(
      process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES,
      'node_modules/@better-auth/core/dist/social-providers/google.mjs'
    )
  )
);
const base = {
  clientId: 'fixture-google-id',
  clientSecret: 'fixture-google-secret',
  redirectURI: 'https://auth.fixture.test/api/auth/callback/google',
  state: 'fixture-state',
  codeVerifier: 'fixture-pkce-verifier'
};
const inputs = [],
  expected = [];
async function compare(patch) {
  const input = { ...base, ...patch };
  inputs.push(input);
  const provider = google({
    clientId: input.clientId,
    clientSecret: input.clientSecret,
    redirectURI: input.redirectURI
  });
  try {
    expected.push({
      url: String(
        await provider.createAuthorizationURL({
          state: input.state,
          codeVerifier: input.codeVerifier,
          scopes: input.scopes,
          loginHint: input.loginHint,
          additionalParams: input.additionalParams
        })
      )
    });
  } catch (error) {
    expected.push({ error: error.message });
  }
}
await compare({});
for (const codeVerifier of ['', 'a', 'A'.repeat(128), 'café+/?~ ', '🙂'])
  await compare({ codeVerifier });
for (const state of ['', 'a b&c=~', '雪🙂']) await compare({ state });
for (const scopes of [
  [],
  ['openid'],
  ['one', 'two'],
  ['a b', '', 'a b'],
  ['雪']
])
  await compare({ scopes });
for (const loginHint of ['', 'fixture@example.test', 'a b&c=~', '雪🙂'])
  await compare({ loginHint });
for (const redirectURI of [
  'https://auth.fixture.test/callback?x=1&y=~',
  'http://localhost:3000/api/auth/callback/google',
  'https://auth.fixture.test//api/auth/callback/google'
])
  await compare({ redirectURI });
for (const field of [
  'state',
  'client_id',
  'redirect_uri',
  'response_type',
  'code_challenge',
  'code_challenge_method',
  'nonce',
  'scope'
])
  await compare({
    additionalParams: { [field]: 'must-not-override', hd: 'example.test' }
  });
for (const additionalParams of [
  { hd: 'example.test' },
  { include_granted_scopes: 'false' },
  { include_granted_scopes: '', login_hint: 'overridden' },
  { prompt: 'select_account', access_type: 'offline', display: 'popup' },
  { 2: 'two', 1: 'one', a: '雪 &~' },
  { constructor: 'ordinary', toString: 'ordinary' }
])
  await compare({ additionalParams, loginHint: 'original' });
await compare({ clientId: '' });
await compare({ clientSecret: '' });
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
    'google_authorize_parity'
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    input: inputs.map((value) => JSON.stringify(value)).join('\n') + '\n',
    encoding: 'utf8',
    maxBuffer: 4 * 1024 * 1024
  }
);
assert.equal(run.status, 0, run.stderr);
const results = run.stdout
  .trim()
  .split('\n')
  .map((line) => JSON.parse(line));
assert.equal(results.length, expected.length);
for (let i = 0; i < results.length; i++)
  assert.deepEqual(results[i], expected[i], JSON.stringify(inputs[i]));
console.log(
  `Better Auth Google authorization URL: ${inputs.length} actual provider comparisons passed`
);
