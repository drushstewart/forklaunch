// Actual configured provider account selection, refresh and account-info HTTP.
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
async function authorize(who, body = {}, route = '/sign-in/social') {
  // Random provider state/challenge are compared separately from the common URL.
  const a = await reference.auth.handler(
    new Request(`http://localhost:3000/api/auth${route}`, {
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
    path: route,
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
async function sync() {
  await native({
    op: 'replace-seed',
    now,
    users: JSON.parse(JSON.stringify(reference.database.user)),
    accounts: JSON.parse(JSON.stringify(reference.database.account)),
    sessions: JSON.parse(JSON.stringify(reference.database.session)),
    verifications: JSON.parse(JSON.stringify(reference.database.verification))
  });
}
async function tokenRequest(who, path, body, expected) {
  const before = actualExchanges.length;
  const result = await request(who, path, path, {
    method: body === undefined ? 'GET' : 'POST',
    ...(body === undefined ? {} : { body: JSON.stringify(body) })
  });
  if (expected !== undefined) assert.equal(result.status, expected);
  assert.equal(nativeExchanges.length, actualExchanges.length);
  if (actualExchanges.length > before) {
    assert.equal(nativeExchanges.at(-1).body, actualExchanges.at(-1).body);
    const form = new URLSearchParams(nativeExchanges.at(-1).body);
    assert.equal(form.get('grant_type'), 'refresh_token');
    assert.equal(form.get('client_id'), 'fixture-google-id');
    assert.equal(form.get('client_secret'), 'fixture-google-secret');
    assert.equal(nativeExchanges.at(-1).headers.accept, 'application/json');
    checks++;
  }
  return result;
}
try {
  const who = client();
  await callback(who, await authorize(who), { code: 'initial-account' });
  await sync();
  who.native = new Map(who.actual);
  const id = reference.database.account[0].id,
    owner = reference.database.user[0].id;
  const account = () => reference.database.account.find((row) => row.id === id);
  const set = async (changes) => {
    Object.assign(account(), changes);
    await sync();
  };
  await tokenRequest(who, '/get-access-token', { accountId: id }, 200);
  await tokenRequest(who, `/account-info?accountId=${id}`, undefined, 200);
  await tokenRequest(who, '/list-accounts', undefined, 200);
  await tokenRequest(client(), '/get-access-token', { accountId: id }, 401);
  await tokenRequest(client(), `/account-info?accountId=${id}`, undefined, 401);
  for (const input of [
    null,
    false,
    0,
    'text',
    [],
    {},
    { accountId: 1 },
    { accountId: id, userId: 1 },
    { accountId: id, extra: 1 },
    { accountId: id, a: 1, b: 2 },
    { useAccountCookie: true, other: 1 },
    { accountId: id, useAccountCookie: true },
    { accountId: 1, useAccountCookie: true },
    { accountId: id, useAccountCookie: false },
    { useAccountCookie: true, userId: null }
  ]) {
    await tokenRequest(who, '/get-access-token', input);
    await tokenRequest(who, '/refresh-token', input);
  }
  for (const selected of ['missing', 'callback-google'])
    await tokenRequest(who, '/get-access-token', { accountId: selected }, 400);
  await tokenRequest(who, '/get-access-token', { useAccountCookie: true }, 400);
  for (const query of [
    '',
    '?useAccountCookie=true',
    `?accountId=${id}&extra=1`,
    `?accountId=${id}&accountId=duplicate`
  ])
    await tokenRequest(who, `/account-info${query}`, undefined, 400);
  reference.database.user.push({
    id: 'foreign-user',
    email: 'foreign@example.test',
    emailVerified: true,
    name: 'Foreign',
    firstName: 'Foreign',
    lastName: 'Person'
  });
  reference.database.account.push({
    id: 'foreign-account',
    providerId: 'google',
    accountId: 'foreign-subject',
    userId: 'foreign-user'
  });
  reference.database.account.push({
    id: 'credential-account',
    providerId: 'credential',
    accountId: owner,
    userId: owner
  });
  await sync();
  for (const route of ['/get-access-token', '/refresh-token']) {
    await tokenRequest(
      who,
      route,
      { accountId: 'foreign-account', userId: 'foreign-user' },
      400
    );
    await tokenRequest(who, route, { accountId: 'credential-account' }, 400);
  }
  await tokenRequest(
    who,
    '/account-info?accountId=foreign-account&userId=foreign-user',
    undefined,
    400
  );
  await tokenRequest(
    who,
    '/account-info?accountId=credential-account',
    undefined,
    400
  );
  await tokenRequest(
    who,
    '/get-access-token',
    { accountId: id, userId: 'foreign-user' },
    200
  );
  tokenData = {
    access_token: 'boundary-refreshed',
    expires_in: 120.5,
    scope: 'narrow'
  };
  for (const offset of [5001, 5000, 4999, 0, -1]) {
    await set({
      accessToken: 'old-boundary',
      refreshToken: 'private-old-refresh',
      accessTokenExpiresAt: new Date(now + offset)
    });
    const before = actualExchanges.length;
    await tokenRequest(who, '/get-access-token', { accountId: id }, 200);
    assert.equal(actualExchanges.length - before, offset < 5000 ? 1 : 0);
    await records();
  }
  // An expired token without a refresh token is returned unchanged by configured TS.
  await set({ accessTokenExpiresAt: new Date(now - 1), refreshToken: null });
  await tokenRequest(who, '/get-access-token', { accountId: id }, 200);
  await tokenRequest(who, '/refresh-token', { accountId: id }, 400);
  await set({ refreshToken: 'private-old-refresh' });
  for (const data of [
    {
      access_token: 'new-access',
      refresh_token: 'rotated-refresh',
      expires_in: 600,
      refresh_token_expires_in: 1200,
      id_token: await token({ name: 'New Profile' }),
      scope: 'narrow'
    },
    { access_token: 'newer-access' },
    { access_token: null, refresh_token: '', id_token: '' },
    {
      access_token: 'nullable-access',
      refresh_token: null,
      id_token: null,
      expires_in: 'bad',
      refresh_token_expires_in: 'bad'
    },
    {},
    null,
    [],
    'text'
  ]) {
    tokenData = data;
    await tokenRequest(
      who,
      '/refresh-token',
      { accountId: id },
      data === null ? 400 : 200
    );
    await records();
  }
  tokenData = {
    access_token: 'automatic',
    id_token: '',
    refresh_token: '',
    expires_in: 3600
  };
  await set({
    accessToken: 'before-empty',
    idToken: await token(),
    accessTokenExpiresAt: new Date(now),
    refreshToken: 'still-private'
  });
  await tokenRequest(who, '/get-access-token', { accountId: id }, 200);
  await records();
  for (const value of ['error', 'redirect', 'malformed']) {
    mode = value;
    await tokenRequest(
      who,
      '/refresh-token',
      { accountId: id },
      value === 'malformed' ? 200 : 400
    );
    await records();
  }
  mode = 'ok';
  tokenData = {
    access_token: 'fresh-info',
    id_token: await token({
      name: 'Refreshed Profile',
      given_name: 'Refreshed'
    }),
    expires_in: 3600
  };
  await set({ accessTokenExpiresAt: new Date(now) });
  await tokenRequest(who, `/account-info?accountId=${id}`, undefined, 200);
  await records();
  await set({ accessToken: '', accessTokenExpiresAt: null });
  await tokenRequest(who, `/account-info?accountId=${id}`, undefined, 400);
  await set({ accessToken: 'present', idToken: null });
  await tokenRequest(who, `/account-info?accountId=${id}`, undefined, 401);
  await set({ idToken: 'not-a-token' });
  await tokenRequest(who, `/account-info?accountId=${id}`, undefined, 500);
  await set({
    idToken: await token(),
    accessToken: 'valid',
    accessTokenExpiresAt: null
  });
  for (const scope of [
    null,
    false,
    0,
    '',
    true,
    1,
    [],
    {},
    ' email,\ufeffprofile,,email '
  ]) {
    await set({ scope });
    await tokenRequest(who, '/get-access-token', { accountId: id });
  }
  for (const body of [
    { accountId: id, 2: 'two', 1: 'one' },
    JSON.parse(`{"accountId":"${id}","__proto__":1}`),
    { accountId: id, constructor: 1 },
    { accountId: id, toString: 1 }
  ]) {
    await tokenRequest(who, '/get-access-token', body);
  }
  console.log(
    `Configured Better Auth Google account tokens: ${checks} actual response/storage/wire comparisons passed; ${nativeExchanges.length} provider exchanges`
  );
} finally {
  globalThis.fetch = originalFetch;
  globalThis.Date = RealDate;
  child.stdin.end();
  await completion;
  await new Promise((resolve) => server.close(resolve));
}
