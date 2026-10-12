// Compare public configured responses with the actual Better Auth HTTP handler.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const reference = await betterAuthReference(root);
const child = spawn(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    'blueprint-rs/Cargo.toml',
    '-p',
    'forklaunch-iam-better-auth',
    '--features',
    'http',
    '--example',
    'http_parity'
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    stdio: ['pipe', 'pipe', 'inherit']
  }
);
const pending = [];
createInterface({ input: child.stdout }).on('line', (line) =>
  pending.shift().resolve(JSON.parse(line))
);
const completion = new Promise((resolve, reject) => {
  child.once('error', reject);
  child.once('exit', (code) => {
    if (code === 0) resolve();
    else reject(new Error(`Public bridge exited ${code}`));
    for (const p of pending.splice(0))
      p.reject(new Error('Public bridge closed'));
  });
});
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
let checks = 0;
try {
  const queries = [
    '',
    '?error=',
    '?error=A_B-123',
    '?error=quote%27code',
    '?error=bad%3Cscript%3E',
    '?error=%C3%A9',
    '?error=first&error=second',
    '?error=&error=second',
    '?error=SPACE+CODE',
    '?error_description=hello',
    '?error_description=',
    '?error_description=%3Cscript%3E%22%27%26',
    '?error_description=%26amp%3B%26lt%3B%26quot%3B%26gt%3B%26%2339%3B',
    '?error_description=%26%23xAf09%3B%26%23123%3B%26%23X12%3B%26%23x%3B%26bogus%3B',
    '?error_description=FL_ERROR_CODE+FL_ERROR_DESCRIPTION+FL_ERROR_DISPLAY+FL_ERROR_QUESTION',
    '?error_description=%F0%9F%90%B1&error_description=other',
    '?error_description=&error_description=ignored',
    '?error=__proto__',
    '?error=FL_ERROR_DESCRIPTION',
    '?error=FL_ERROR_CODE',
    '?error=FL_ERROR_DISPLAY',
    '?error=FL_ERROR_QUESTION',
    '?error=%00',
    '?error_description=%26%26amp%3B%26%23123%3B%26',
    '?error_description=%EF%BF%BD'
  ];
  for (const query of queries) {
    const route = `/error${query}`;
    const actual = await reference.auth.handler(
      new Request(`http://localhost:3000/api/auth${route}`)
    );
    const response = await native({
      op: 'request',
      now: Date.now(),
      method: 'GET',
      path: route,
      headers: {},
      body: ''
    });
    assert.equal(response.status, actual.status, route);
    assert.equal(
      response.headers['content-type']?.[0] ?? null,
      actual.headers.get('content-type'),
      route
    );
    if (actual.headers.has('location'))
      assert.equal(
        response.headers.location?.[0],
        actual.headers.get('location'),
        route
      );
    assert.equal(response.body, await actual.text(), route);
    checks++;
  }
  for (const route of ['/reference', '/open-api/generate-schema']) {
    const actual = await reference.auth.handler(
      new Request(`http://localhost:3000/api/auth${route}`)
    );
    const response = await native({
      op: 'request',
      now: Date.now(),
      method: 'GET',
      path: route,
      headers: {},
      body: ''
    });
    assert.equal(response.status, actual.status, route);
    assert.equal(
      response.headers['content-type']?.[0] ?? null,
      actual.headers.get('content-type'),
      route
    );
    const text = await actual.text();
    const expected = text ? JSON.parse(text) : '';
    assert.deepEqual(response.body, expected, route);
    if (route.includes('generate-schema')) {
      const methods = Object.values(expected.paths).flatMap((path) =>
        Object.keys(path)
      );
      assert.ok(
        methods.length >= 70,
        'configured public schema must contain all route operations'
      );
    }
    checks++;
  }
  console.log(
    `Configured Better Auth public HTTP: ${checks} actual full-response comparisons passed (${process.env.NODE_ENV ?? 'development'})`
  );
} finally {
  child.stdin.end();
  await completion;
}
