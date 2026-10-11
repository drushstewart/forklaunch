// Compare native primitives directly with Better Auth 1.7.6 and Better Call.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const actual = (file) =>
  import(pathToFileURL(path.join(dependencies, 'node_modules', file)));
const [passwords, cookies, { createInternalContext }] = await Promise.all([
  actual('@better-auth/utils/dist/password.mjs'),
  actual('better-call/dist/cookies.mjs'),
  actual('better-call/dist/context.mjs'),
]);
const child = spawn(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    'blueprint-rs/Cargo.toml',
    '-p',
    'forklaunch-iam-better-auth',
    '--example',
    'crypto_parity',
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    stdio: ['pipe', 'pipe', 'inherit'],
  },
);
const pending = [];
const lines = createInterface({ input: child.stdout });
lines.on('line', (line) => {
  const next = pending.shift();
  assert.ok(next, `Unexpected native output: ${line}`);
  next.resolve(JSON.parse(line));
});
const completion = new Promise((resolve, reject) => {
  child.once('error', reject);
  child.once('exit', (code) => {
    if (code === 0) resolve();
    else reject(new Error(`Native crypto bridge exited ${code}`));
    for (const waiting of pending.splice(0))
      waiting.reject(new Error('Native bridge closed'));
  });
});
// Always observe early compiler failures, even when awaiting a bridge response.
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
let checks = 0;
try {
  for (const [password, equivalent] of [
    ['test-only-password', 'test-only-password'],
    ['caf\u00e9-\u212b', 'cafe\u0301-A\u030a'],
    ['\uff26\uff4f\uff52\uff4b\ufb03\u2460', 'Forkffi1'],
    ['\ud83d\udd10\u0000pass\u2028word', '\ud83d\udd10\u0000pass\u2028word'],
  ]) {
    const hash = await passwords.hashPassword(password);
    assert.equal(
      await native({ op: 'verify', password: equivalent, hash }),
      true,
    );
    assert.equal(
      await native({ op: 'verify', password: `${equivalent}!`, hash }),
      false,
    );
    const rustHash = await native({ op: 'hash', password });
    assert.match(rustHash, /^[a-f0-9]{32}:[a-f0-9]{128}$/);
    assert.equal(await passwords.verifyPassword(rustHash, equivalent), true);
    assert.equal(
      await passwords.verifyPassword(rustHash, `${equivalent}!`),
      false,
    );
    assert.equal(
      await native({ op: 'verify', password, hash: `${hash}:ignored` }),
      true,
    );
    const [salt, key] = hash.split(':');
    assert.equal(
      await native({
        op: 'verify',
        password,
        hash: `${salt}:${key.toUpperCase()}`,
      }),
      false,
    );
    checks += 7;
  }
  for (const hash of [
    '',
    'plaintext',
    ':key',
    'salt:',
    'salt:00',
    'salt:'.concat('g'.repeat(128)),
  ]) {
    assert.equal(
      await native({ op: 'verify', password: 'secret', hash }),
      false,
    );
    checks++;
  }
  const secret = 'fixture-cookie-secret-32-characters';
  for (const value of [
    'opaque-session-token',
    'value.with.dots',
    'unicode-\u00e9-\ud83d\udd10',
    "a b/+%=!'()*~",
    '',
    'line\nseparator',
  ]) {
    for (const secure of [false, true]) {
      const name = `${secure ? '__Secure-' : ''}better-auth.session_token`;
      const options = { path: '/', httpOnly: true, secure, sameSite: 'lax' };
      for (const persistent of [false, true]) {
        const expected = await cookies.serializeSignedCookie(
          name,
          value,
          secret,
          {
            ...options,
            ...(persistent ? { maxAge: 86400 } : {}),
          },
        );
        const result = await native({
          op: 'serialize',
          value,
          secret,
          secure,
          persistent,
        });
        assert.equal(result.header, expected);
        assert.equal(result.name, name);
        assert.equal(
          result.clear,
          cookies.serializeCookie(name, '', { ...options, maxAge: 0 }),
        );
        checks += 3;
      }
      const signed = (await cookies.serializeSignedCookie(name, value, secret))
        .split(';')[0]
        .slice(name.length + 1);
      assert.equal(await native({ op: 'sign', value, secret }), signed);
      for (const header of [
        `${name}=${signed}`,
        `other=x; ${name}="${signed}"; ${name}=duplicate`,
        `${name}=invalid; ${name}=${signed}`,
        `${name}=${signed}forged`,
        `${name}=%FF`,
        `${name}=%GG`,
        `${name}=%`,
        `${name}=valid%20then%GG`,
        `other=${signed}`,
      ]) {
        const expectedValue = cookies.parseCookies(header).get(name) ?? null;
        const context = await createInternalContext(
          { headers: new Headers({ cookie: header }) },
          { options: { method: 'GET' }, path: '/fixture' },
        );
        const expectedToken = await context.getSignedCookie(name, secret);
        const result = await native({ op: 'cookie', name, secret, header });
        assert.equal(result.value, expectedValue);
        assert.equal(result.token, expectedToken || null);
        checks += 2;
      }
    }
  }
  console.log(
    `Better Auth native password/cookie parity: ${checks} comparisons passed`,
  );
} finally {
  child.stdin.end();
  await completion;
  lines.close();
}
