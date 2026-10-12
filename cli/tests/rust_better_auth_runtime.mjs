import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHmac, createPublicKey, randomUUID, verify } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import http from 'node:http';
import { probeMcp, mcpRpc } from './rust_mcp_probe.mjs';
const [binary, sdkPath] = process.argv.slice(2);
const { createForkLaunchSdk } = await import(pathToFileURL(sdkPath));
const role = process.env.FORKLAUNCH_TEST_ROLE;
const database = new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);
database.username = role;
database.password = 'parity-test-only';
const secret = 'generated-better-auth-hmac-fixture';
const origin = 'http://localhost:8000';
const processes = [];
let publishedKeys = { keys: [] };
const jwksServer = http.createServer((_request, response) => {
  response.setHeader('content-type', 'application/json');
  response.end(JSON.stringify(publishedKeys));
});
await new Promise((resolve) => jwksServer.listen(0, '127.0.0.1', resolve));
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const inherited = Object.fromEntries(
  Object.entries(process.env).filter(
    ([key]) =>
      !key.startsWith('OTEL_') &&
      !key.startsWith('MCP_') &&
      ![
        'JWT_ISSUER',
        'JWT_AUDIENCE',
        'JWKS_PUBLIC_KEY_URL',
        'GOOGLE_CLIENT_ID',
        'GOOGLE_CLIENT_SECRET'
      ].includes(key)
  )
);
async function start(tenant, overrides = {}) {
  const child = spawn(binary, [], {
    env: {
      ...inherited,
      DATABASE_URL: database.toString(),
      TENANT_ID: `${role}-${tenant}`,
      ENCRYPTION_KEY: 'generated-auth-encryption-fixture-only',
      HMAC_SECRET_KEY: secret,
      JWKS_PUBLIC_KEY_URL: `http://127.0.0.1:${jwksServer.address().port}/jwks`,
      BETTER_AUTH_URL: origin,
      BETTER_AUTH_SECRET: 'generated-auth-cookie-fixture-secret-32-characters',
      HOST: '127.0.0.1',
      SERVICE_PORT: '0',
      NODE_ENV: 'development',
      ...overrides
    },
    stdio: ['ignore', 'pipe', 'pipe']
  });
  let stdout = '',
    stderr = '',
    closed = false;
  child.stdout.on('data', (data) => {
    stdout += data;
  });
  child.stderr.on('data', (data) => {
    stderr += data;
  });
  const exited = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code) => {
      closed = true;
      resolve(code);
    });
  });
  exited.catch(() => {});
  const process = {
    child,
    exited,
    get closed() {
      return closed;
    }
  };
  processes.push(process);
  for (let index = 0; index < 300; index++) {
    assert.ok(!closed, `Generated auth exited: ${stderr}`);
    const base = stdout.match(/^iam listening on (http:\/\/[^\s]+)/m)?.[1];
    if (base) {
      try {
        const health = await fetch(`${base}/health`, {
          signal: AbortSignal.timeout(500)
        });
        if (health.ok)
          return {
            ...process,
            base,
            mcp:
              stdout.match(/^MCP listening on (http:\/\/[^\s]+)/m)?.[1] ?? base
          };
      } catch {
        /* Wait until the managed listeners are serving. */
      }
    }
    await delay(50);
  }
  throw Error(`Generated auth startup timed out: ${stdout}\n${stderr}`);
}
async function stop(process) {
  process.child.kill('SIGTERM');
  let timeout;
  try {
    assert.equal(
      await Promise.race([
        process.exited,
        new Promise((_, reject) => {
          timeout = setTimeout(
            () => reject(Error('Generated auth shutdown timed out')),
            5000
          );
        })
      ]),
      0
    );
  } finally {
    clearTimeout(timeout);
  }
  await assert.rejects(
    fetch(`${process.base}/health`, { signal: AbortSignal.timeout(500) })
  );
}
function hmac(path) {
  const timestamp = new Date().toISOString(),
    nonce = randomUUID();
  const signature = createHmac('sha256', secret)
    .update(`GET\n${path}\nundefined${timestamp}\n${nonce}`)
    .digest('base64');
  return `HMAC keyId=default ts=${timestamp} nonce=${nonce} signature=${signature}`;
}
async function call(base, path, options = {}, status = 200) {
  const response = await fetch(base + path, {
    ...options,
    signal: AbortSignal.timeout(10000)
  });
  assert.equal(response.status, status, await response.clone().text());
  return response;
}
try {
  const first = await start('a');
  const second = await start('b', {
    MCP_LISTENER: 'separate',
    MCP_PORT: '0',
    MCP_PATH: '/tools'
  });
  for (const service of [first, second]) {
    const docs = await call(service.base, '/docs');
    assert.equal(docs.headers.get('content-type'), 'text/html; charset=utf-8');
    const html = await docs.text();
    const config = JSON.parse(
      html
        .split("Scalar.createApiReference('#app', ")[1]
        .split(');</script>')[0]
    );
    assert.deepEqual(config.sources[0], {
      title: 'API Reference',
      url: '/openapi.json'
    });
    assert.deepEqual(config.sources[1], {
      title: 'BetterAuth',
      content: await (
        await call(service.base, '/api/auth/open-api/generate-schema')
      ).json()
    });
    assert.equal(config.sources.length, 2);
    assert.ok(config.sources[1].content.paths['/get-session']);
    assert.equal(
      config.sources[1].content.servers[0].url,
      `${origin}/api/auth`
    );
    const application = await (
      await call(service.base, config.sources[0].url)
    ).json();
    assert.equal(Object.keys(application.paths).length, 6);
  }
  assert.equal(
    (
      await probeMcp(first.base, [
        'getauthmethods',
        'surfaceUserRoles',
        'surfaceUserPermissions',
        'exportuserdata',
        'eraseuserdata'
      ])
    ).length,
    6
  );
  assert.equal(
    (await probeMcp(second.mcp, ['getauthmethods'], '/tools', second.base))
      .length,
    6
  );
  const sdk = createForkLaunchSdk({ baseUrl: first.base });
  const discovery = await sdk.iam.discovery.getauthmethods();
  assert.equal(discovery.code, 200);
  assert.equal(discovery.response.email, true);
  assert.deepEqual(discovery.response.socialProviders, ['google']);
  const mcpDiscovery = await mcpRpc(first.base, 'tools/call', {
    name: 'getauthmethods',
    arguments: {}
  });
  assert.deepEqual(mcpDiscovery.structuredContent, discovery.response);
  const signup = await call(first.base, '/api/auth/sign-up/email', {
    method: 'POST',
    headers: { 'content-type': 'application/json', origin },
    body: JSON.stringify({
      email: 'generated@example.test',
      password: 'generated-fixture-password',
      name: 'Generated User',
      firstName: 'Generated',
      lastName: 'User'
    })
  });
  const identity = await signup.json();
  const cookie = signup.headers
    .getSetCookie()
    .filter((value) => value.startsWith('better-auth.session_token='))
    .map((value) => value.split(';')[0])
    .join('; ');
  assert.ok(cookie);
  const session = await call(first.base, '/api/auth/get-session', {
    headers: { cookie }
  });
  assert.equal((await session.json()).user.id, identity.user.id);
  const token = session.headers.get('set-auth-jwt');
  const jwks = await (await call(first.base, '/api/auth/jwks')).json();
  const popup = await call(
    first.base,
    '/api/auth/test-callback?callbackUrl=%2Fpopup%3Fx%3D1%23old',
    { headers: { cookie }, redirect: 'manual' },
    302
  );
  const destination = new URL(popup.headers.get('location'));
  assert.equal(destination.origin, origin);
  assert.equal(destination.pathname, '/popup');
  assert.equal(destination.search, '?x=1');
  const fragment = new URLSearchParams(destination.hash.slice(1));
  assert.equal(fragment.get('sessionToken'), identity.token);
  assert.equal(fragment.get('email'), 'generated@example.test');
  assert.ok(fragment.get('token'));
  assert.equal(popup.headers.getSetCookie().length, 1);
  const page = await call(
    first.base,
    '/api/auth/oauth-redirect?callbackURL=%2Fpopup&provider=google'
  );
  assert.match(await page.text(), /credentials: 'include'/);
  await call(
    first.base,
    '/api/auth/test-callback?callbackUrl=https%3A%2F%2Fevil.example.test',
    {},
    400
  );
  publishedKeys = jwks;
  const [header, payload, signature] = token.split('.');
  const key = jwks.keys.find(
    (key) => key.kid === JSON.parse(Buffer.from(header, 'base64url')).kid
  );
  assert.ok(
    verify(
      null,
      Buffer.from(`${header}.${payload}`),
      createPublicKey({ key, format: 'jwk' }),
      Buffer.from(signature, 'base64url')
    )
  );
  assert.equal(
    JSON.parse(Buffer.from(payload, 'base64url')).sub,
    identity.user.id
  );
  assert.equal(
    await (
      await call(second.base, '/api/auth/get-session', { headers: { cookie } })
    ).json(),
    null
  );
  for (const suffix of ['surface-roles', 'surface-permissions']) {
    const path = `/user/${identity.user.id}/${suffix}`;
    await call(first.base, path, {}, 401);
    const authorization = hmac(path);
    assert.deepEqual(
      await (
        await call(first.base, path, { headers: { authorization } })
      ).json(),
      []
    );
    const result = await mcpRpc(first.base, 'tools/call', {
      name:
        suffix === 'surface-roles'
          ? 'surfaceUserRoles'
          : 'surfaceUserPermissions',
      arguments: {
        params: { id: identity.user.id },
        headers: { authorization }
      }
    });
    assert.equal(result.isError, false);
    assert.deepEqual(JSON.parse(result.content[0].text), []);
  }
  for (const [path, method] of [
    ['export', 'GET'],
    ['erase', 'DELETE']
  ]) {
    await call(
      first.base,
      `/compliance/${path}/${identity.user.id}`,
      { method },
      401
    );
    await call(
      first.base,
      `/compliance/${path}/${identity.user.id}`,
      { method, headers: { authorization: `Bearer ${token}` } },
      403
    );
  }
  await stop(first);
  const restarted = await start('a', { MCP_ENABLED: 'false' });
  assert.equal(
    (
      await (
        await call(restarted.base, '/api/auth/get-session', {
          headers: { cookie }
        })
      ).json()
    ).user.id,
    identity.user.id
  );
  await call(
    restarted.base,
    '/mcp',
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{}'
    },
    404
  );
  await stop(restarted);
  await stop(second);
  console.log(
    'PASS generated Better Auth HTTP/MCP shared/separate/disabled listeners, SDK discovery, encrypted tenant sessions, independent JWT verification, HMAC tools, role denial, restart and shutdown'
  );
} finally {
  for (const process of processes)
    if (!process.closed) process.child.kill('SIGKILL');
  await Promise.allSettled(processes.map((process) => process.exited));
  await new Promise((resolve) => jwksServer.close(resolve));
}
