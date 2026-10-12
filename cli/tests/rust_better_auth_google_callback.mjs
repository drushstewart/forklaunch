// Real configured Google code callbacks beside native production HTTP/storage.
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
const { privateKey } = await jose.generateKeyPair('RS256');
const RealDate = Date;
let now = Math.floor(Date.now() / 1000) * 1000;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    super(...(args.length ? args : [now]));
  }
  static now() {
    return now;
  }
};
const profile = {
  sub: 'callback-google',
  email: 'callback@example.test',
  email_verified: true,
  name: 'Callback Person',
  given_name: 'Callback',
  family_name: 'Person'
};
async function token(changes = {}) {
  return new jose.SignJWT({ ...profile, ...changes })
    .setProtectedHeader({ alg: 'RS256' })
    .sign(privateKey);
}
let tokenData = {
    access_token: 'private-code-access',
    refresh_token: 'private-code-refresh',
    expires_in: 3600,
    refresh_token_expires_in: 7200,
    scope: 'openid email profile',
    id_token: await token()
  },
  mode = 'ok';
const actualExchanges = [],
  nativeExchanges = [];
const server = createServer(async (req, res) => {
  assert.equal(req.url, '/token');
  assert.equal(req.method, 'POST');
  let body = '';
  for await (const chunk of req) body += chunk;
  nativeExchanges.push({ body, headers: req.headers });
  if (mode === 'redirect') {
    res.writeHead(302, { location: '/never-follow' });
    res.end();
    return;
  }
  res.writeHead(mode === 'error' ? 400 : 200, {
    'content-type': 'application/json'
  });
  res.end(
    mode === 'malformed'
      ? '{bad'
      : mode === 'large'
        ? ' '.repeat(1024 * 1024 + 1)
        : JSON.stringify(tokenData)
  );
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const endpoint = `http://127.0.0.1:${server.address().port}/token`;
const originalFetch = globalThis.fetch;
globalThis.fetch = async (input, init) => {
  const url =
    typeof input === 'string'
      ? input
      : input instanceof URL
        ? input.href
        : input.url;
  assert.equal(
    url,
    'https://oauth2.googleapis.com/token',
    'callback must use trusted token endpoint without certificate fetch'
  );
  assert.equal(init.method, 'POST');
  actualExchanges.push({ body: String(init.body), headers: init.headers });
  if (mode === 'redirect')
    return new Response('', {
      status: 302,
      headers: { location: '/never-follow' }
    });
  return new Response(
    mode === 'malformed' ? '{bad' : JSON.stringify(tokenData),
    {
      status: mode === 'error' ? 400 : 200,
      headers: { 'content-type': 'application/json' }
    }
  );
};
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
      FORKLAUNCH_GOOGLE_CERTIFICATES_URL: `http://127.0.0.1:${server.address().port}/certs`,
      FORKLAUNCH_GOOGLE_TOKEN_URL: endpoint
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
    else reject(new Error(`Callback bridge exited ${code}`));
    for (const p of pending.splice(0))
      p.reject(new Error('Callback bridge closed'));
  });
});
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
const cookie = (jar) =>
  [...jar].map(([name, value]) => `${name}=${value}`).join('; ');
const cookies = (jar, lines) => {
  for (const line of lines) {
    const [pair] = line.split(';'),
      i = pair.indexOf('='),
      name = pair.slice(0, i),
      value = pair.slice(i + 1);
    if (value) jar.set(name, value);
    else jar.delete(name);
  }
};
function client() {
  return { actual: new Map(), native: new Map() };
}
let checks = 0;
async function request(
  who,
  actualPath,
  nativePath = actualPath,
  { method = 'GET', body, headers = {} } = {}
) {
  const common = {
    origin: 'http://localhost:3000',
    'content-type': 'application/json',
    ...headers
  };
  const a = await reference.auth.handler(
    new Request(`http://localhost:3000/api/auth${actualPath}`, {
      method,
      headers: { ...common, cookie: cookie(who.actual) },
      ...(body === undefined ? {} : { body })
    })
  );
  const result = await native({
    op: 'request',
    now,
    method,
    path: nativePath,
    headers: { ...common, cookie: cookie(who.native) },
    body: body ?? ''
  });
  const text = await a.text();
  let actualBody = text;
  try {
    actualBody = JSON.parse(text);
  } catch {}
  assert.equal(
    result.status,
    a.status,
    JSON.stringify({
      actualPath,
      body,
      actual: actualBody,
      native: result.body
    })
  );
  if (a.headers.has('location'))
    assert.equal(result.headers.location?.[0], a.headers.get('location'));
  else if (!actualBody.url) assert.deepEqual(result.body, actualBody);
  cookies(who.actual, a.headers.getSetCookie());
  cookies(who.native, result.headers['set-cookie'] ?? []);
  assert.deepEqual(
    [...who.native.keys()].sort(),
    [...who.actual.keys()].sort()
  );
  checks++;
  return {
    actual: actualBody,
    result,
    status: a.status,
    location: a.headers.get('location')
  };
}
async function authorize(who, body = {}) {
  // Random provider state/challenge are compared separately from the common URL.
  const a = await reference.auth.handler(
    new Request('http://localhost:3000/api/auth/sign-in/social', {
      method: 'POST',
      headers: {
        origin: 'http://localhost:3000',
        'content-type': 'application/json',
        cookie: cookie(who.actual)
      },
      body: JSON.stringify({
        provider: 'google',
        callbackURL: '/done',
        errorCallbackURL: '/failure?existing=~#fragment',
        newUserCallbackURL: '/welcome',
        ...body
      })
    })
  );
  const actual = await a.json();
  assert.equal(a.status, 200);
  const result = await native({
    op: 'request',
    now,
    method: 'POST',
    path: '/sign-in/social',
    headers: {
      origin: 'http://localhost:3000',
      'content-type': 'application/json',
      cookie: cookie(who.native)
    },
    body: JSON.stringify({
      provider: 'google',
      callbackURL: '/done',
      errorCallbackURL: '/failure?existing=~#fragment',
      newUserCallbackURL: '/welcome',
      ...body
    })
  });
  assert.equal(result.status, 200);
  cookies(who.actual, a.headers.getSetCookie());
  cookies(who.native, result.headers['set-cookie'] ?? []);
  const actualURL = new URL(actual.url),
    nativeURL = new URL(result.body.url);
  return {
    actual: actualURL.searchParams.get('state'),
    native: nativeURL.searchParams.get('state'),
    actualChallenge: actualURL.searchParams.get('code_challenge'),
    nativeChallenge: nativeURL.searchParams.get('code_challenge')
  };
}
async function callback(who, state, params = {}, provider = 'google') {
  const query = (extra) => new URLSearchParams({ state: extra, ...params });
  const before = actualExchanges.length;
  const result = await request(
    who,
    `/callback/${provider}?${query(state.actual)}`,
    `/callback/${provider}?${query(state.native)}`
  );
  assert.equal(nativeExchanges.length, actualExchanges.length);
  if (actualExchanges.length > before) {
    const a = new URLSearchParams(actualExchanges.at(-1).body),
      b = new URLSearchParams(nativeExchanges.at(-1).body);
    for (const [body, challenge] of [
      [a, state.actualChallenge],
      [b, state.nativeChallenge]
    ]) {
      assert.equal(body.get('grant_type'), 'authorization_code');
      assert.equal(body.get('client_id'), 'fixture-google-id');
      assert.equal(body.get('client_secret'), 'fixture-google-secret');
      assert.equal(
        body.get('redirect_uri'),
        'http://localhost:3000/api/auth/callback/google'
      );
      assert.equal(body.get('code'), params.code);
      assert.equal(
        await jose.base64url.encode(
          new Uint8Array(
            await crypto.subtle.digest(
              'SHA-256',
              new TextEncoder().encode(body.get('code_verifier'))
            )
          )
        ),
        challenge
      );
      body.delete('code_verifier');
    }
    assert.equal(b.toString(), a.toString());
    checks++;
  }
  return result;
}
const omit = (value, fields) =>
  Object.fromEntries(
    Object.entries(value).filter(([key]) => !fields.includes(key))
  );
async function records() {
  const saved = await native({ op: 'records', now });
  for (const [name, actual] of [
    ['users', 'user'],
    ['accounts', 'account'],
    ['sessions', 'session']
  ]) {
    assert.deepEqual(
      saved[name].map((row) => ({
        ...omit(row, ['id', 'userId', 'token']),
        ...('userId' in row
          ? { owner: saved.users.findIndex((user) => user.id === row.userId) }
          : {})
      })),
      JSON.parse(JSON.stringify(reference.database[actual])).map((row) => ({
        ...omit(row, ['id', 'userId', 'token']),
        ...('userId' in row
          ? {
              owner: reference.database.user.findIndex(
                (user) => user.id === row.userId
              )
            }
          : {})
      })),
      name
    );
    checks++;
  }
  return saved;
}
async function linkState(state, userIndex = 0) {
  const saved = await native({ op: 'records', now, withVerifications: true });
  const actualRow = reference.database.verification.find(
    (row) => row.identifier === state.actual
  );
  actualRow.value = JSON.stringify({
    ...JSON.parse(actualRow.value),
    link: {
      userId: reference.database.user[userIndex].id,
      email: reference.database.user[userIndex].email
    }
  });
  const row = saved.verifications.find(
    (row) => row.identifier === state.native
  );
  row.value = JSON.stringify({
    ...JSON.parse(row.value),
    link: {
      userId: saved.users[userIndex].id,
      email: saved.users[userIndex].email
    }
  });
  await native({ ...saved, op: 'replace-seed', now });
}
try {
  await request(client(), '/callback/google');
  await request(client(), '/callback/google?code=unbound');
  await request(client(), '/callback/google?state=missing&code=unbound');
  const who = client(),
    state = await authorize(who);
  const stolen = client();
  await callback(stolen, state, { code: 'cannot-consume' });
  await callback(who, state, { code: 'valid-code', iss: 'ignored-for-google' });
  await records();
  await callback(who, state, { code: 'replay' });
  const existing = client();
  await callback(existing, await authorize(existing), {
    code: 'existing-account'
  });
  await records();
  for (const params of [
    {
      error: 'access_denied',
      error_description: 'Denied + by provider & user'
    },
    {},
    { code: '' }
  ]) {
    const next = client();
    await callback(next, await authorize(next), params);
  }
  for (const value of ['error', 'redirect', 'malformed']) {
    mode = value;
    const next = client();
    await callback(next, await authorize(next), { code: 'failed-code' });
  }
  mode = 'ok';
  for (const data of [
    {},
    { access_token: 'only-access' },
    { id_token: await token({ email: undefined }) },
    { id_token: await token({ sub: null }) }
  ]) {
    tokenData = data;
    const next = client();
    await callback(next, await authorize(next), { code: 'missing-profile' });
  }
  tokenData = {
    id_token: await token(),
    access_token: 'second-code-access',
    scope: [' email ', '', 'email', 12, 'calendar'],
    expires_in: '120.5',
    refresh_token_expires_in: 0
  };
  const next = client();
  await callback(next, await authorize(next), { code: 'updated-tokens' });
  await records();
  for (const expires of [
    undefined,
    null,
    false,
    true,
    0,
    1,
    -1,
    0.125,
    '0',
    '0x10',
    ' 2.5 ',
    [],
    [3],
    {},
    'bad',
    1e100
  ]) {
    tokenData = {
      id_token: await token(),
      access_token: 'expiry-access',
      expires_in: expires,
      scope: '\ufeff email\tprofile\n email '
    };
    const expiry = client();
    await callback(expiry, await authorize(expiry), {
      code: 'expiry-normalization'
    });
    await records();
  }
  for (const scope of [
    undefined,
    null,
    false,
    12,
    '',
    [],
    [' email ', '\ufeffprofile', null, {}, 'email']
  ]) {
    tokenData = {
      id_token: await token({
        sub: `scope-${checks}`,
        email: `scope-${checks}@example.test`
      }),
      scope
    };
    const scoped = client();
    await callback(scoped, await authorize(scoped), {
      code: 'scope-normalization'
    });
    await records();
  }
  for (const email_verified of [undefined, null, false, 0, 'yes']) {
    tokenData = {
      id_token: await token({
        sub: `verified-${checks}`,
        email: `verified-${checks}@example.test`,
        email_verified
      })
    };
    const verified = client();
    await callback(verified, await authorize(verified), {
      code: 'verification-profile'
    });
    await records();
  }
  for (const changes of [
    {},
    { sub: 'explicit-new' },
    { email: 'mismatch@example.test' },
    { email_verified: false },
    { sub: 'scope-conflict' }
  ]) {
    if (changes.sub === 'scope-conflict') {
      tokenData = {
        id_token: await token({
          sub: 'scope-conflict',
          email: 'conflict@example.test'
        })
      };
      const signup = client();
      await callback(signup, await authorize(signup), {
        code: 'create-conflicting-owner'
      });
    }
    tokenData = {
      id_token: await token(changes),
      access_token: 'explicit-access',
      scope: ['calendar', 'email', 'literal,scope', 'literal']
    };
    const linked = client(),
      state = await authorize(linked);
    await linkState(state);
    await callback(linked, state, { code: 'explicit-link' });
    await records();
  }
  const unknown = client();
  await callback(
    unknown,
    await authorize(unknown),
    { code: 'unknown-provider' },
    'absent'
  );
  // POST only normalizes schema/query into GET, without consuming state or exchanging code.
  await request(
    client(),
    '/callback/google?code=query-wins&state=query-state',
    undefined,
    {
      method: 'POST',
      body: JSON.stringify({
        code: 'body-code',
        state: 'body-state',
        user: '{"name":"test"}',
        extra: 'stripped'
      })
    }
  );
  await request(client(), '/callback/google?state=from-query', undefined, {
    method: 'POST',
    body: 'code=form+code&state=body-state&error_description=hello%2Bworld',
    headers: { 'content-type': 'application/x-www-form-urlencoded' }
  });
  for (const path of [
    '/callback/google?state=a&state=b',
    '/callback/google?code=a&code=b'
  ])
    await request(client(), path);
  for (const body of [
    null,
    [],
    { code: 1, state: null, error_description: false }
  ])
    await request(client(), '/callback/google', undefined, {
      method: 'POST',
      body: JSON.stringify(body)
    });
  await request(client(), '/callback/google', undefined, {
    method: 'POST',
    body: JSON.stringify({ code: 'external' }),
    headers: { origin: 'https://evil.test' }
  });
  for (const id_token of ['not-a-token', 'a.e30.c', 'a.W10.c', null, 0]) {
    tokenData = { id_token };
    const invalid = client();
    await callback(invalid, await authorize(invalid), {
      code: 'malformed-id-token'
    });
  }
  tokenData = { id_token: await token() };
  const beforeExpiry = now;
  for (const age of [600000, 600001]) {
    now = beforeExpiry;
    const expired = client(),
      state = await authorize(expired);
    now += age;
    await callback(expired, state, { code: 'state-expiry-boundary' });
  }
  now = beforeExpiry;
  console.log(
    `Configured Better Auth Google callbacks: ${checks} actual response/storage/wire comparisons passed; ${nativeExchanges.length} code exchanges`
  );
} finally {
  globalThis.fetch = originalFetch;
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
  await new Promise((resolve) => server.close(resolve));
}
