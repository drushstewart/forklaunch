// Direct conformance with Better Auth 1.7.6 trusted origin matcher.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const { matchesOriginPattern } = await import(
  pathToFileURL(
    path.join(
      dependencies,
      'node_modules/better-auth/dist/auth/trusted-origins.mjs',
    ),
  )
);
const patterns = [
  'http://localhost:3000',
  'https://example.com',
  'https://EXAMPLE.com',
  'https://example.com/',
  'https://*.example.com',
  '*.example.com',
  'https://?.example.com',
  'https://**/example.com',
  '*',
  '**',
  'myapp://',
  'myapp://host',
  'MYAPP://HOST/cb',
  'myapp://host/cb',
  'myapp://host/cb/',
  'myapp://host/cb/../other',
  'myapp://host/cb%2finner',
  'myapp://host/cb/%ZZ',
  'myapp://*/cb',
  'myapp://host/c?',
  'myapp://host/??',
  'myapp://host/🔐',
  'ftp://example.com/path',
  'javascript:',
  'invalid',
  '/callback',
  'https://*.example.com:443',
  'https://*.example.com:8443',
];
const values = [
  'http://localhost:3000',
  'http://localhost:3000/callback',
  'http://localhost:3001',
  'https://example.com',
  'https://EXAMPLE.COM/path?x=1',
  'https://example.com:443/path',
  'https://a.example.com',
  'https://a.b.example.com',
  'https://a.example.com:8443',
  'https://evil.test',
  'https://example.com.evil.test',
  'https://example.com@evil.test',
  'https://evil.test@example.com',
  'https://[::1]:3000',
  '//evil.test',
  '/callback',
  '/a/../callback?next=https://evil.test',
  '/%2f/evil.test',
  '/%5Cevil.test',
  '/callback?next=%2f%2fevil.test',
  '/\\evil.test',
  '/\u0000evil.test',
  '/\u0085evil.test',
  'myapp://host/cb',
  'myapp://HOST/cb/inner',
  'MYAPP://host/cb?x=1',
  'myapp://host/cb/../evil',
  'myapp://host/cb/%2e%2e/evil',
  'myapp://host/cb%2f..%2fevil',
  'myapp://host/cb\\..\\evil',
  'myapp://host/cb%2finner',
  'myapp://host/cb/%ZZ',
  'myapp://host/cb/%ff',
  'myapp://host/cb/%0a',
  'myapp://other/cb',
  'myapp://host/🔐',
  'myapp://host/cb\n',
  'myapp:/cb',
  'javascript:alert(1)',
  'ftp://example.com/path/inner',
  'ftp://example.com/path/../evil',
  'invalid',
  '',
  ' https://example.com ',
  'https://exam\nple.com',
  'https://example.com\\evil.test',
  'https://example.com/%2f%2f',
  '/%FF',
  '/%ZZ',
];
const cases = patterns.flatMap((pattern) =>
  values.flatMap((value) =>
    [false, true].map((relative) => ({ pattern, value, relative })),
  ),
);
const expected = cases.map(({ pattern, value, relative }) =>
  matchesOriginPattern(value, pattern, { allowRelativePaths: relative }),
);
const reference = await betterAuthReference(root);
const headerCases = [
  {},
  { cookie: '' },
  { cookie: 'unrelated=value' },
  { origin: 'http://localhost:3000' },
  { origin: 'https://evil.test' },
  { origin: 'null' },
  { referer: 'http://localhost:3000/dashboard' },
  { referer: 'https://evil.test/path' },
  { cookie: '', origin: 'http://localhost:3000' },
  { cookie: '', origin: 'https://evil.test' },
  { cookie: '', referer: 'http://localhost:3000/dashboard' },
  { cookie: '', origin: 'null' },
  { cookie: '', origin: 'null', 'sec-fetch-site': 'same-origin' },
  { origin: 'null', 'sec-fetch-site': 'same-origin' },
  {
    'sec-fetch-site': 'cross-site',
    'sec-fetch-mode': 'navigate',
    'sec-fetch-dest': 'document',
  },
  {
    origin: 'http://localhost:3000',
    'sec-fetch-site': 'cross-site',
    'sec-fetch-mode': 'navigate',
  },
  { 'sec-fetch-site': 'cross-site', 'sec-fetch-mode': 'cors' },
  {
    origin: 'http://localhost:3000',
    'sec-fetch-site': 'cross-site',
    'sec-fetch-mode': 'cors',
  },
  { 'sec-fetch-site': 'same-origin' },
  { 'sec-fetch-mode': 'cors' },
  { 'sec-fetch-dest': 'empty' },
  { 'sec-fetch-site': ' ' },
  { origin: '', referer: 'http://localhost:3000/page' },
  { origin: 'https://evil.test', referer: 'http://localhost:3000/page' },
  { cookie: '', origin: 'http://localhost:3000.evil.test' },
];
const guards = [false, true].flatMap((login) =>
  headerCases.map((headers) => ({
    op: 'guard',
    login,
    headers,
    body: login
      ? { email: 'absent@example.test', password: 'x'.repeat(129) }
      : {},
  })),
);
for (const field of [
  'callbackURL',
  'redirectTo',
  'errorCallbackURL',
  'newUserCallbackURL',
])
  for (const value of [
    '/dashboard',
    '//evil.test',
    '/%2f/evil.test',
    '/dashboard?next=%2f%2fevil.test',
    'https://evil.test',
    'http://localhost:3000/dashboard',
    '',
  ])
    guards.push({
      op: 'guard',
      login: true,
      headers: {},
      body: {
        email: 'absent@example.test',
        password: 'x'.repeat(129),
        [field]: value,
      },
    });
for (const guard of guards) {
  const response = await reference.auth.handler(
    new Request(
      `http://localhost:3000/api/auth${guard.login ? '/sign-in/email' : '/sign-out'}`,
      {
        method: 'POST',
        headers: { 'content-type': 'application/json', ...guard.headers },
        body: JSON.stringify(guard.body),
      },
    ),
  );
  const body = await response.json();
  const allowed = response.status === 200 || body.code === 'PASSWORD_TOO_LONG';
  assert.ok(
    allowed || response.status === 403,
    JSON.stringify({ guard, status: response.status, body }),
  );
  cases.push(guard);
  expected.push(allowed ? 'allowed' : body.code);
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
    'origins_parity',
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    input: JSON.stringify(cases),
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
  },
);
assert.equal(run.status, 0, run.stderr);
const actual = JSON.parse(run.stdout);
for (let i = 0; i < cases.length; i++)
  assert.equal(actual[i], expected[i], JSON.stringify(cases[i]));
console.log(
  `Better Auth native origin matcher: ${cases.length} actual-library comparisons passed`,
);
