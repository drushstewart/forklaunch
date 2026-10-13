// Native persistent key selection/rotation compared with the actual JWT plugin.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createPrivateKey } from 'node:crypto';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const dependencies = process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies, 'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const actual = (file) =>
  import(pathToFileURL(path.join(dependencies, 'node_modules', file)));
const [{ getJwtToken }, jose, reference] = await Promise.all([
  actual('better-auth/dist/plugins/jwt/sign.mjs'),
  actual('jose/dist/webapi/index.js'),
  betterAuthReference(root),
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
    'keyring_parity',
  ],
  {
    cwd: root,
    env: { ...process.env, CARGO_INCREMENTAL: '0' },
    stdio: ['pipe', 'pipe', 'inherit'],
  },
);
const pending = [];
const lines = createInterface({ input: child.stdout });
lines.on('line', (line) => pending.shift().resolve(JSON.parse(line)));
const completion = new Promise((resolve, reject) => {
  child.once('error', reject);
  child.once('exit', (code) => {
    if (code === 0) resolve();
    else reject(new Error(`Keyring fixture exited ${code}`));
    for (const next of pending.splice(0))
      next.reject(new Error('Native bridge closed'));
  });
});
completion.catch(() => {});
const native = (input) =>
  new Promise((resolve, reject) => {
    pending.push({ resolve, reject });
    child.stdin.write(`${JSON.stringify(input)}\n`);
  });
const RealDate = Date;
async function atTime(now, work) {
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [now * 1000]));
    }
    static now() {
      return now * 1000;
    }
  };
  try {
    return await work();
  } finally {
    globalThis.Date = RealDate;
  }
}
function actualRow(row) {
  const encoded = Buffer.from(row.privateKey, 'base64');
  assert.equal(
    encoded.subarray(0, 16).toString('hex'),
    '3051020101300506032b657004220420',
  );
  const key = createPrivateKey({
    key: Buffer.concat([
      Buffer.from('302e020100300506032b657004220420', 'hex'),
      encoded.subarray(16, 48),
    ]),
    format: 'der',
    type: 'pkcs8',
  }).export({ format: 'jwk' });
  return {
    ...row,
    privateKey: JSON.stringify(key),
    createdAt: new RealDate(row.createdAt),
    ...(row.expiresAt ? { expiresAt: new RealDate(row.expiresAt) } : {}),
  };
}
const start = Math.floor(RealDate.now() / 1000);
let checks = 0;
async function compare(now) {
  const keys = await native({ op: 'jwks', now });
  const rows = (await native({ op: 'records', now })).map(actualRow);
  reference.database.jwks.splice(0, reference.database.jwks.length, ...rows);
  const actualKeys = await atTime(now, () => reference.client().call('/jwks'));
  assert.equal(actualKeys.status, 200);
  assert.deepEqual(keys, actualKeys.body);
  checks += 2;
  return { keys, rows };
}
try {
  const initial = await compare(start);
  assert.equal(initial.keys.keys.length, 1);
  const first = await native({ op: 'issue', now: start });
  assert.equal(await native({ op: 'issue', now: start }), first);
  const rotated = await native({ op: 'rotate', now: start + 10 });
  assert.notEqual(rotated.kid, initial.keys.keys[0].kid);
  assert.equal((await compare(start + 10)).keys.keys.length, 2);
  const token = await native({ op: 'issue', now: start + 10 });
  const options = {
    ...reference.config.plugins.find((plugin) => plugin.id === 'jwt').options,
    jwks: { disablePrivateKeyEncryption: true },
    adapter: { getJwks: async () => reference.database.jwks },
  };
  const context = {
    context: {
      options: { baseURL: 'https://auth.fixture.test' },
      session: {
        user: { id: 'fixture-user', email: 'fixture@example.test' },
        session: { activeOrganizationId: 'fixture-org' },
      },
    },
  };
  const actualToken = await atTime(start + 10, () =>
    getJwtToken(context, options),
  );
  const publicKey = await jose.importJWK(rotated, 'EdDSA');
  const verification = {
    algorithms: ['EdDSA'],
    issuer: 'https://auth.fixture.test',
    audience: 'https://auth.fixture.test',
    currentDate: new RealDate((start + 10) * 1000),
  };
  const decoded = await jose.jwtVerify(token, publicKey, verification);
  const actualDecoded = await jose.jwtVerify(
    actualToken,
    publicKey,
    verification,
  );
  assert.deepEqual(decoded, actualDecoded);
  await compare(start + 10 + 30 * 86400 - 1);
  assert.deepEqual((await compare(start + 10 + 30 * 86400)).keys.keys, [
    rotated,
  ]);
  const rows = await native({ op: 'records', now: start });
  const live = rows.find((row) => row.id === rotated.kid);
  // Native rotate retires live keys before provisioning the replacement.
  await native({ op: 'rotate', now: start + 20 });
  const all = await native({ op: 'records', now: start });
  const newest = all.find((row) => !row.expiresAt);
  // Add another live key at the same timestamp to exercise stable insertion ties.
  const tie = { ...live, id: 'fixture-tie-key', createdAt: newest.createdAt };
  const tiePublic = JSON.parse(tie.publicKey);
  tiePublic.kid = tie.id;
  tie.publicKey = JSON.stringify(tiePublic);
  delete tie.expiresAt;
  await native({ op: 'seed', now: start, row: tie });
  const tieToken = await native({ op: 'issue', now: start + 20 });
  assert.equal(jose.decodeProtectedHeader(tieToken).kid, newest.id);
  checks += 8;
  await compare(start + 20);
  const actualTieToken = await atTime(start + 20, () =>
    getJwtToken(context, options),
  );
  assert.equal(jose.decodeProtectedHeader(actualTieToken).kid, newest.id);
  await native({ op: 'retire', now: start + 30 });
  assert.deepEqual((await compare(start + 30 + 30 * 86400)).keys.keys, []);
  const previousCount = (await native({ op: 'records', now: start })).length;
  const reinitialized = await native({
    op: 'issue',
    now: start + 30 + 30 * 86400,
  });
  assert.notEqual(jose.decodeProtectedHeader(reinitialized).kid, newest.id);
  assert.equal(
    (await native({ op: 'records', now: start })).length,
    previousCount + 1,
  );
  assert.equal((await compare(start + 30 + 30 * 86400)).keys.keys.length, 1);
  checks += 5;
  // The configured adapter limits findMany to its first 100 inserted records.
  // Publication must apply that limit before filtering expired keys.
  const boundaryTime = start + 30 + 30 * 86400;
  const boundaryRows = await native({ op: 'records', now: boundaryTime });
  const template = boundaryRows.find((row) => !row.expiresAt);
  for (let index = boundaryRows.length; index < 101; index++) {
    const row = { ...template, id: `fixture-boundary-${index}` };
    const publicKey = JSON.parse(row.publicKey);
    publicKey.kid = row.id;
    row.publicKey = JSON.stringify(publicKey);
    await native({ op: 'seed', now: boundaryTime, row });
  }
  const boundary = await compare(boundaryTime);
  assert.ok(!boundary.keys.keys.some((key) => key.kid === 'fixture-boundary-100'));
  checks++;
  console.log(
    `Better Auth native key lifecycle parity: ${checks} comparisons passed`,
  );
} finally {
  child.stdin.end();
  await completion;
  lines.close();
  globalThis.Date = RealDate;
}
