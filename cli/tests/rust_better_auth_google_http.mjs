// Actual configured Google HTTP handlers beside native production routes/storage.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const reference = await betterAuthReference(root, { google: true });
const jose = await import(
  pathToFileURL(
    path.join(
      process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES,
      'node_modules/jose/dist/webapi/index.js'
    )
  )
);
const { publicKey, privateKey } = await jose.generateKeyPair('RS256', {
  extractable: true
});
const publicJwk = {
  ...(await jose.exportJWK(publicKey)),
  kid: 'google-fixture',
  alg: 'RS256',
  use: 'sig'
};
const unrelated = await jose.generateKeyPair('RS256', { extractable: true });
const wrongJwk = {
  ...(await jose.exportJWK(unrelated.publicKey)),
  kid: publicJwk.kid,
  alg: 'RS256'
};
let keys = [publicJwk],
  mode = 'ok',
  requests = 0;
const server = createServer((req, res) => {
  assert.equal(req.url, '/certs');
  requests++;
  if (mode === 'redirect') {
    res.writeHead(302, { location: '/never-follow' });
    res.end();
    return;
  }
  res.writeHead(mode === 'error' ? 503 : 200, {
    'content-type': 'application/json'
  });
  res.end(
    mode === 'malformed'
      ? '{bad'
      : mode === 'large'
        ? ' '.repeat(1024 * 1024 + 1)
        : JSON.stringify({ keys })
  );
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
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
    env: {
      ...process.env,
      CARGO_INCREMENTAL: '0',
      FORKLAUNCH_GOOGLE_CERTIFICATES_URL: `http://127.0.0.1:${server.address().port}/certs`
    },
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
    else reject(new Error(`Google bridge exited ${code}`));
    for (const p of pending.splice(0))
      p.reject(new Error('Google bridge closed'));
  });
});
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
const RealDate = Date,
  now = Math.floor(Date.now() / 1000) * 1000;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [now]));
  }
  static now() {
    return now;
  }
};
const originalFetch = globalThis.fetch;
globalThis.fetch = async (input) => {
  const url =
    typeof input === 'string'
      ? input
      : input instanceof URL
        ? input.href
        : input.url;
  assert.equal(url, 'https://www.googleapis.com/oauth2/v3/certs');
  if (mode === 'redirect')
    return new Response('', {
      status: 302,
      headers: { location: '/never-follow' }
    });
  return new Response(
    mode === 'malformed'
      ? '{bad'
      : mode === 'large'
        ? ' '.repeat(1024 * 1024 + 1)
        : JSON.stringify({ keys }),
    {
      status: mode === 'error' ? 503 : 200,
      headers: { 'content-type': 'application/json' }
    }
  );
};
const profile = {
  sub: 'google-subject',
  email: 'google@example.test',
  email_verified: true,
  name: 'Google Person',
  given_name: 'Google',
  family_name: 'Person',
  picture: 'https://fixture.test/avatar.png',
  nonce: 'fixture-nonce'
};
async function token(changes = {}, header = {}) {
  return new jose.SignJWT({
    ...profile,
    iat: now / 1000,
    exp: now / 1000 + 300,
    iss: 'https://accounts.google.com',
    aud: 'fixture-google-id',
    ...changes
  })
    .setProtectedHeader({ alg: 'RS256', kid: publicJwk.kid, ...header })
    .sign(privateKey);
}
const omit = (value, fields) =>
  Object.fromEntries(
    Object.entries(value).filter(([key]) => !fields.includes(key))
  );
let checks = 0;
function client() {
  return { actual: reference.client(), cookies: new Map() };
}
function cookie(who) {
  return [...who.cookies].map(([name, value]) => `${name}=${value}`).join('; ');
}
async function call(who, body, path = '/sign-in/social', expectedStatus) {
  const actual = await who.actual.call(path, body);
  const result = await native({
    op: 'request',
    now,
    method: body === undefined ? 'GET' : 'POST',
    path,
    headers: {
      origin: 'http://localhost:3000',
      'content-type': 'application/json',
      cookie: cookie(who)
    },
    body: body === undefined ? '' : JSON.stringify(body)
  });
  assert.equal(
    result.status,
    actual.status,
    JSON.stringify({ body, actual: actual.body, native: result.body })
  );
  if (expectedStatus !== undefined)
    assert.equal(actual.status, expectedStatus, JSON.stringify(actual.body));
  if (actual.body.user) {
    assert.deepEqual(
      omit(result.body.user, ['id']),
      omit(actual.body.user, ['id'])
    );
    assert.ok(result.body.token ?? result.body.session.token);
  } else if (actual.body.url) {
    const a = new URL(actual.body.url),
      b = new URL(result.body.url);
    for (const key of ['state', 'code_challenge']) {
      assert.ok(b.searchParams.get(key));
      a.searchParams.delete(key);
      b.searchParams.delete(key);
    }
    assert.equal(b.href, a.href);
    assert.equal(result.body.redirect, actual.body.redirect);
    assert.equal(
      Boolean(result.headers.location),
      actual.headers.has('location')
    );
  } else assert.deepEqual(result.body, actual.body);
  for (const line of result.headers['set-cookie'] ?? []) {
    const [pair] = line.split(';'),
      split = pair.indexOf('='),
      name = pair.slice(0, split),
      value = pair.slice(split + 1);
    if (value) who.cookies.set(name, value);
    else who.cookies.delete(name);
  }
  checks++;
  return { actual, result };
}
async function records() {
  const saved = await native({ op: 'records', now });
  for (const [nativeName, actualName] of [
    ['users', 'user'],
    ['accounts', 'account'],
    ['sessions', 'session']
  ]) {
    assert.deepEqual(
      saved[nativeName].map((row) => ({
        ...omit(row, ['id', 'userId', 'token']),
        ...('userId' in row
          ? { owner: saved.users.findIndex((user) => user.id === row.userId) }
          : {})
      })),
      JSON.parse(JSON.stringify(reference.database[actualName])).map((row) => ({
        ...omit(row, ['id', 'userId', 'token']),
        ...('userId' in row
          ? {
              owner: reference.database.user.findIndex(
                (user) => user.id === row.userId
              )
            }
          : {})
      })),
      nativeName
    );
    checks++;
  }
  return saved;
}
async function seed() {
  await native({
    op: 'replace-seed',
    now,
    users: JSON.parse(JSON.stringify(reference.database.user)),
    accounts: JSON.parse(JSON.stringify(reference.database.account)),
    sessions: JSON.parse(JSON.stringify(reference.database.session))
  });
}
try {
  const who = client(),
    signed = await token();
  await call(
    who,
    {
      provider: 'google',
      idToken: {
        token: signed,
        nonce: profile.nonce,
        accessToken: 'first-access',
        refreshToken: 'not-persisted',
        expiresAt: 1
      }
    },
    undefined,
    200
  );
  await call(who, undefined, '/get-session', 200);
  await records();
  await call(
    client(),
    {
      provider: 'google',
      idToken: {
        token: await token({
          given_name: 'Changed',
          name: 'Changed',
          email: 'changed@example.test'
        }),
        accessToken: 'second-access'
      }
    },
    undefined,
    200
  );
  await records();
  for (const changes of [
    { aud: 'wrong' },
    { iss: 'wrong' },
    { exp: now / 1000 - 1 },
    { iat: now / 1000 - 3601 },
    { iat: now / 1000 + 1 },
    { nbf: now / 1000 + 1 }
  ])
    await call(
      client(),
      { provider: 'google', idToken: { token: await token(changes) } },
      undefined,
      401
    );
  await call(
    client(),
    { provider: 'google', idToken: { token: signed, nonce: 'wrong' } },
    undefined,
    401
  );
  await call(
    client(),
    {
      provider: 'google',
      idToken: { token: await token({}, { kid: 'missing' }) }
    },
    undefined,
    401
  );
  const parts = signed.split('.'),
    forged = Buffer.from(parts[2], 'base64url');
  forged[0] ^= 1;
  parts[2] = forged.toString('base64url');
  await call(
    client(),
    { provider: 'google', idToken: { token: parts.join('.') } },
    undefined,
    401
  );
  for (const subject of [
    undefined,
    null,
    '',
    '  ',
    'undefined',
    'null',
    { toString: 1 }
  ])
    await call(
      client(),
      { provider: 'google', idToken: { token: await token({ sub: subject }) } },
      undefined,
      401
    );
  for (const email of [undefined, null, '', false])
    await call(
      client(),
      { provider: 'google', idToken: { token: await token({ email }) } },
      undefined,
      401
    );
  for (const changes of [
    {
      sub: 'no-names',
      email: 'names@example.test',
      given_name: undefined,
      family_name: undefined
    },
    { sub: 42, email: 'number@example.test', given_name: 1, family_name: [] },
    {
      sub: ['array', 'subject'],
      email: 'array@example.test',
      name: undefined,
      picture: undefined
    },
    { sub: { id: 1 }, email: 'object@example.test', email_verified: false }
  ]) {
    await call(
      client(),
      { provider: 'google', idToken: { token: await token(changes) } },
      undefined,
      200
    );
    await records();
  }
  // Centralized configured verifier uses the first selected key, after importing all.
  keys = [wrongJwk, publicJwk];
  await call(
    client(),
    { provider: 'google', idToken: { token: signed } },
    undefined,
    401
  );
  keys = [publicJwk, wrongJwk];
  await call(
    client(),
    { provider: 'google', idToken: { token: signed } },
    undefined,
    200
  );
  keys = [publicJwk, { kid: publicJwk.kid, kty: 'invalid' }];
  await call(
    client(),
    { provider: 'google', idToken: { token: signed } },
    undefined,
    401
  );
  keys = [publicJwk];
  for (const value of ['error', 'malformed', 'redirect']) {
    mode = value;
    await call(
      client(),
      { provider: 'google', idToken: { token: signed } },
      undefined,
      401
    );
  }
  mode = 'ok';
  // Native bounds the owned HTTP boundary even when upstream accepts oversized JSON.
  mode = 'large';
  const before = (await native({ op: 'records', now })).sessions.length;
  const bounded = await native({
    op: 'request',
    now,
    method: 'POST',
    path: '/sign-in/social',
    headers: {
      origin: 'http://localhost:3000',
      'content-type': 'application/json'
    },
    body: JSON.stringify({ provider: 'google', idToken: { token: signed } })
  });
  assert.equal(bounded.status, 401);
  assert.equal((await native({ op: 'records', now })).sessions.length, before);
  checks++;
  mode = 'ok';
  for (const body of [
    {},
    { provider: 'absent' },
    { provider: 'google', idToken: 'bad' },
    { provider: 12 },
    { provider: 'google', scopes: [1, null] },
    {
      provider: 'google',
      idToken: {
        token: 12,
        nonce: 4,
        expiresAt: 'bad',
        user: { name: { firstName: 1 }, email: 2 }
      }
    },
    { provider: 'google', additionalParams: { state: 'override' } },
    { provider: 'google', additionalParams: { foo: 1 } },
    { provider: 'google', additionalData: [] }
  ])
    await call(client(), body);
  for (const value of [null, false, 0, [], {}, 'text']) {
    for (const field of [
      'provider',
      'disableRedirect',
      'idToken',
      'scopes',
      'requestSignUp',
      'loginHint',
      'additionalParams',
      'additionalData'
    ]) {
      await call(client(), { provider: 'google', [field]: value });
    }
    for (const field of [
      'token',
      'nonce',
      'accessToken',
      'refreshToken',
      'expiresAt',
      'user'
    ]) {
      await call(client(), {
        provider: 'google',
        idToken: { token: 'not-a-token', [field]: value }
      });
    }
  }
  for (const reserved of [
    'state',
    'client_id',
    'redirect_uri',
    'response_type',
    'code_challenge',
    'code_challenge_method',
    'nonce',
    'scope'
  ]) {
    await call(client(), {
      provider: 'google',
      additionalParams: { [reserved]: 'override' }
    });
  }
  for (const body of [
    { provider: 'google' },
    {
      provider: 'google',
      disableRedirect: true,
      callbackURL: '/done',
      newUserCallbackURL: '/welcome',
      errorCallbackURL: '/error',
      scopes: ['calendar', 'email'],
      loginHint: 'hello+test@example.test',
      additionalParams: { prompt: 'consent', 1: 'one' },
      additionalData: {
        hello: 'world',
        link: { userId: 'attacker' },
        idTokenNonce: 'attacker',
        callbackURL: 'https://attacker.test'
      }
    }
  ]) {
    const next = client();
    await call(next, body, undefined, 200);
    const saved = await native({ op: 'records', now, withVerifications: true });
    const state = JSON.parse(saved.verifications.at(-1).value);
    assert.equal(state.link, undefined);
    assert.equal(state.idTokenNonce, undefined);
    assert.equal(
      state.callbackURL,
      body.callbackURL ?? 'http://localhost:3000'
    );
    const actualState = JSON.parse(
      reference.database.verification.at(-1).value
    );
    assert.deepEqual(
      omit(state, ['codeVerifier', 'oauthState']),
      omit(actualState, ['codeVerifier', 'oauthState'])
    );
    checks++;
  }
  // Verified-email implicit linking; unverified local identity remains protected.
  for (const localVerified of [false, true]) {
    const email = `local-${localVerified}@example.test`,
      user = {
        id: `local-${localVerified}`,
        email,
        name: 'Local',
        firstName: 'Local',
        lastName: 'Person',
        emailVerified: localVerified,
        createdAt: new Date(now),
        updatedAt: new Date(now)
      };
    reference.database.user.push(user);
    await seed();
    await call(
      client(),
      {
        provider: 'google',
        idToken: {
          token: await token({ sub: `linked-${localVerified}`, email })
        }
      },
      undefined,
      localVerified ? 200 : 401
    );
    await records();
  }
  // Seed synchronization replaces native random tokens; establish fresh paired cookies.
  await call(
    who,
    { provider: 'google', idToken: { token: signed } },
    undefined,
    200
  );
  await call(
    client(),
    { provider: 'google', idToken: { token: signed } },
    '/link-social',
    401
  );
  await call(who, { provider: 'absent' }, '/link-social', 404);
  await call(
    who,
    { provider: 'google', idToken: { token: signed, nonce: 'wrong' } },
    '/link-social',
    401
  );
  await call(
    who,
    {
      provider: 'google',
      idToken: {
        token: signed,
        accessToken: 'direct-linked-access',
        refreshToken: 'direct-linked-refresh',
        expiresAt: 'ignored',
        user: { name: 12 }
      }
    },
    '/link-social',
    200
  );
  await records();
  await call(
    who,
    {
      provider: 'google',
      idToken: {
        token: await token({
          email_verified: false,
          email: 'changed-own@example.test'
        })
      }
    },
    '/link-social',
    200
  );
  await records();
  for (const changes of [
    { sub: 'direct-additional' },
    { sub: 'direct-unverified', email_verified: false },
    { sub: 'direct-different-email', email: 'foreign@example.test' },
    { sub: 'no-email', email: undefined }
  ]) {
    await call(
      who,
      {
        provider: 'google',
        idToken: { token: await token(changes), refreshToken: 'linked-refresh' }
      },
      '/link-social'
    );
    await records();
  }
  const foreignLinked = client();
  await call(
    foreignLinked,
    {
      provider: 'google',
      idToken: {
        token: await token({
          sub: 'foreign-direct',
          email: 'foreign-direct@example.test'
        })
      }
    },
    undefined,
    200
  );
  await call(
    who,
    {
      provider: 'google',
      idToken: {
        token: await token({
          sub: 'foreign-direct',
          email: 'google@example.test'
        })
      }
    },
    '/link-social',
    409
  );
  await records();
  for (const value of [null, false, 0, [], {}, 'text']) {
    for (const field of [
      'provider',
      'idToken',
      'callbackURL',
      'requestSignUp',
      'scopes',
      'errorCallbackURL',
      'disableRedirect',
      'loginHint',
      'additionalParams',
      'additionalData'
    ]) {
      await call(who, { provider: 'google', [field]: value }, '/link-social');
    }
  }
  await call(
    who,
    {
      provider: 'google',
      callbackURL: '/linked',
      disableRedirect: true,
      scopes: ['calendar'],
      additionalData: {
        link: { userId: 'attacker', email: 'attacker@example.test' }
      },
      newUserCallbackURL: '/ignored'
    },
    '/link-social',
    200
  );
  const linkedState = (
    await native({ op: 'records', now, withVerifications: true })
  ).verifications.at(-1);
  const linkedData = JSON.parse(linkedState.value),
    actualLinked = JSON.parse(reference.database.verification.at(-1).value);
  assert.equal(linkedData.link.email, actualLinked.link.email);
  assert.equal(
    linkedData.link.userId,
    (await native({ op: 'records', now })).users[0].id
  );
  assert.equal(actualLinked.link.userId, reference.database.user[0].id);
  assert.equal(linkedData.newUserURL, undefined);
  assert.deepEqual(
    omit(linkedData, ['codeVerifier', 'oauthState', 'link']),
    omit(actualLinked, ['codeVerifier', 'oauthState', 'link'])
  );
  checks++;
  await call(client(), {}, '/update-session', 401);
  for (const body of [
    {},
    { token: 'forged', userId: 'foreign', expiresAt: '9999-01-01' },
    null,
    [],
    false,
    12,
    'text'
  ]) {
    await call(who, body, '/update-session');
  }
  for (const field of ['activeOrganizationId', 'activeTeamId']) {
    for (const value of [null, false, 0, '', true, 1, 'foreign', [], {}]) {
      await call(who, { [field]: value }, '/update-session');
    }
  }
  await records();
  console.log(
    `Configured Better Auth Google HTTP: ${checks} actual response/storage checks passed; ${requests} bounded native certificate requests`
  );
} finally {
  globalThis.fetch = originalFetch;
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
  await new Promise((resolve) => server.close(resolve));
}
