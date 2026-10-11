// Audit the actual configured plugin surface before porting its protocol.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createPublicKey, verify } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthReference } from './fixtures/rust_better_auth_reference.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const reference = await betterAuthReference(root);
const { config, database, routes, serverOnly, client, discovery, surfacing } =
  reference;
assert.deepEqual(
  routes,
  JSON.parse(
    await readFile(
      path.join(root, 'cli/tests/fixtures/rust_better_auth_routes.json'),
      'utf8',
    ),
  ),
);
assert.equal(routes.length, 70);
assert.deepEqual(serverOnly, [
  'addMember',
  'setPassword',
  'signJWT',
  'verifyJWT',
]);
const orgOptions = config.plugins.find(
  (plugin) => plugin.id === 'organization',
).options;
assert.equal(orgOptions.membershipLimit, 100);
assert.equal(orgOptions.invitationExpiresIn, 48 * 60 * 60);
assert.equal(orgOptions.teams.enabled, true);
assert.equal(orgOptions.dynamicAccessControl.enabled, true);
assert.deepEqual(
  Array.from(config.plugins, (plugin) => plugin.id),
  ['jwt', 'open-api', 'organization'],
);
assert.equal(config.advanced.database.generateId, false);
assert.deepEqual(JSON.parse(JSON.stringify(config.session)), {
  expiresIn: 86400,
  updateAge: 3600,
});
const methods = await discovery();
assert.deepEqual(methods, {
  status: 200,
  body: {
    email: true,
    username: false,
    magicLink: false,
    emailOtp: false,
    phoneNumber: false,
    passkey: false,
    anonymous: false,
    sso: false,
    genericOAuth: false,
    twoFactor: false,
    socialProviders: ['google'],
  },
});
assert.equal(config.socialProviders.google.enabled, false);
let count = 0;
async function expect(who, route, body, status = 200, code, request = {}) {
  const response = await who.call(route, body, request);
  assert.equal(
    response.status,
    status,
    `${route}: ${JSON.stringify(response.body)}`,
  );
  if (code) assert.equal(response.body.code, code, route);
  count++;
  return response;
}
const owner = client(),
  other = client();
assert.equal((await expect(owner, '/get-session')).body, null);
await expect(owner, '/token', undefined, 401, 'UNAUTHORIZED');
await expect(
  owner,
  '/sign-up/email',
  {
    name: 'Missing',
    email: 'missing@example.test',
    password: 'test-only-password',
  },
  400,
);
await expect(
  owner,
  '/sign-up/email',
  {
    name: 'Short',
    firstName: 'Short',
    lastName: 'Password',
    email: 'short@example.test',
    password: 'short',
  },
  400,
  'PASSWORD_TOO_SHORT',
);
const credentials = {
  name: 'First Last',
  firstName: 'First',
  lastName: 'Last',
  email: 'owner@example.test',
  password: 'test-only-password',
};
const signup = await expect(owner, '/sign-up/email', credentials);
const user = signup.body.user;
assert.match(user.id, /^[a-f0-9-]{36}$/);
assert.equal(user.emailVerified, false);
assert.equal(user.firstName, 'First');
assert.equal(user.lastName, 'Last');
assert.ok(!Object.hasOwn(user, 'password'));
assert.ok(signup.body.token);
const sessionCookie = signup.headers
  .getSetCookie()
  .find((cookie) => cookie.startsWith('better-auth.session_token='));
assert.match(sessionCookie, /HttpOnly/);
assert.match(sessionCookie, /SameSite=Lax/i);
assert.match(sessionCookie, /Max-Age=86400/i);
const session = (await expect(owner, '/get-session')).body;
assert.equal(session.session.token, signup.body.token);
assert.ok(
  Math.abs(
    Date.parse(session.session.expiresAt) -
      Date.parse(session.session.createdAt) -
      86400 * 1000,
  ) < 1000,
);
assert.equal(session.user.id, user.id);
assert.equal(await surfacing.surfaceRole(user.id), null);
assert.equal((await surfacing.surfacePermissions(user.id)).length, 0);
assert.equal(database.account[0].providerId, 'credential');
assert.ok(database.account[0].password);
assert.ok(!database.account[0].password.includes(credentials.password));
await expect(
  other,
  '/sign-up/email',
  credentials,
  422,
  'USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL',
);
await expect(
  other,
  '/sign-in/email',
  { email: credentials.email, password: 'wrong-test-password' },
  401,
  'INVALID_EMAIL_OR_PASSWORD',
);
await expect(other, '/sign-up/email', {
  ...credentials,
  email: 'other@example.test',
});
const organization = (
  await expect(owner, '/organization/create', {
    name: 'Fixture Org',
    slug: 'fixture-org',
    domain: 'example.test',
  })
).body;
assert.equal(organization.subscription, 'free');
assert.equal(organization.status, 'active');
assert.equal(organization.members[0].role, 'owner');
assert.equal(organization.members[0].userId, user.id);
assert.equal(
  (await expect(owner, '/organization/list')).body[0].id,
  organization.id,
);
const full = (await expect(owner, '/organization/get-full-organization')).body;
assert.equal(full.teams.length, 1);
assert.equal(full.teams[0].name, 'Fixture Org');
assert.equal(full.teams[0].memberCount, 1);
assert.equal(full.members[0].user.id, user.id);
assert.equal(await surfacing.surfaceRole(user.id), 'owner');
assert.equal((await surfacing.surfacePermissions(user.id)).length, 0);
database.organizationRole.push({
  id: 'fixture-role',
  organizationId: organization.id,
  role: 'owner',
  permission: '{"platform":["platform:read"]}',
});
assert.deepEqual(Array.from(await surfacing.surfacePermissions(user.id)), [
  '{"platform":["platform:read"]}',
]);
database.organizationRole.length = 0;
await expect(
  other,
  `/organization/get-full-organization?organizationId=${organization.id}`,
  undefined,
  403,
);
await expect(
  owner,
  '/organization/create-team',
  { name: 'Second Team', organizationId: organization.id },
  403,
  'YOU_ARE_NOT_ALLOWED_TO_CREATE_TEAMS_IN_THIS_ORGANIZATION',
);
await expect(
  owner,
  '/organization/update',
  { organizationId: organization.id, data: { name: 'Renamed' } },
  403,
  'YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_ORGANIZATION',
);
await expect(
  owner,
  '/organization/invite-member',
  {
    email: 'invited@example.test',
    role: 'viewer',
    organizationId: organization.id,
  },
  403,
  'YOU_ARE_NOT_ALLOWED_TO_INVITE_USERS_TO_THIS_ORGANIZATION',
);
await expect(
  owner,
  '/organization/create-role',
  {
    organizationId: organization.id,
    role: 'reviewer',
    permission: { platform: ['platform:read'] },
  },
  403,
  'YOU_ARE_NOT_ALLOWED_TO_CREATE_A_ROLE',
);
assert.deepEqual(
  (
    await expect(owner, '/organization/has-permission', {
      organizationId: organization.id,
      permissions: { platform: ['platform:read'] },
    })
  ).body,
  { error: null, success: true },
);
// An operator-seeded dynamic grant augments the configured static owner role.
// This exercises the real invitation handler without changing role policy.
database.organizationRole.push({
  id: 'fixture-inviter-role',
  organizationId: organization.id,
  role: 'owner',
  permission: '{"invitation":["create"]}',
});
const invitationStarted = Date.now();
const invitation = (
  await expect(owner, '/organization/invite-member', {
    email: 'invited@example.test',
    role: 'viewer',
    organizationId: organization.id,
  })
).body;
assert.equal(invitation.status, 'pending');
assert.equal(invitation.inviterId, user.id);
assert.ok(
  Math.abs(
    Date.parse(invitation.expiresAt) - invitationStarted - 48 * 60 * 60 * 1000,
  ) < 1000,
);
database.organizationRole.length = 0;
const jwks = (await expect(owner, '/jwks')).body;
const jwt = (await expect(owner, '/token')).body.token;
const [header, payload, signature] = jwt.split('.');
const metadata = JSON.parse(Buffer.from(header, 'base64url'));
const claims = JSON.parse(Buffer.from(payload, 'base64url'));
assert.equal(metadata.alg, 'EdDSA');
const key = jwks.keys.find((key) => key.kid === metadata.kid);
assert.ok(key);
assert.ok(!Object.hasOwn(key, 'd'));
assert.equal(
  verify(
    null,
    Buffer.from(`${header}.${payload}`),
    createPublicKey({ key, format: 'jwk' }),
    Buffer.from(signature, 'base64url'),
  ),
  true,
);
assert.equal(claims.sub, user.id);
assert.equal(claims.email, credentials.email);
assert.equal(claims.activeOrganizationId, organization.id);
assert.equal(claims.iss, config.baseURL);
assert.equal(claims.aud, config.baseURL);
assert.equal(claims.exp - claims.iat, 900);
await expect(
  owner,
  '/change-email',
  { newEmail: 'changed@example.test' },
  400,
  'CHANGE_EMAIL_DISABLED',
);
await expect(owner, '/delete-user', {}, 404);
await expect(
  owner,
  '/request-password-reset',
  { email: credentials.email },
  400,
  'RESET_PASSWORD_DISABLED',
);
await expect(
  owner,
  '/sign-in/social',
  { provider: 'google', callbackURL: config.baseURL },
  404,
  'PROVIDER_NOT_FOUND',
);
await expect(owner, '/reference', undefined, 404);
await expect(owner, '/verify-password', { password: credentials.password });
await expect(
  owner,
  '/verify-password',
  { password: 'wrong-test-password' },
  400,
  'INVALID_PASSWORD',
);
const signedCookie = owner.cookies.get('better-auth.session_token');
const forged = client();
forged.cookies.set(
  'better-auth.session_token',
  signedCookie.slice(0, -1) + (signedCookie.endsWith('A') ? 'B' : 'A'),
);
assert.equal((await expect(forged, '/get-session')).body, null);
await expect(owner, '/sign-out', {});
assert.equal((await expect(owner, '/get-session')).body, null);
const replay = client();
replay.cookies.set('better-auth.session_token', signedCookie);
assert.equal((await expect(replay, '/get-session')).body, null);
await expect(owner, '/sign-in/email', {
  email: credentials.email.toUpperCase(),
  password: credentials.password,
});
assert.equal((await expect(owner, '/get-session')).body.user.id, user.id);
const ownerPeer = client();
await expect(ownerPeer, '/sign-in/email', {
  email: credentials.email,
  password: credentials.password,
});
assert.equal((await expect(owner, '/list-sessions')).body.length, 2);
await expect(
  owner,
  '/update-user',
  { name: 'Forbidden' },
  403,
  'INVALID_ORIGIN',
  { headers: { origin: 'http://evil.example.test' } },
);
await expect(
  owner,
  '/update-user',
  {
    name: 'Updated Name',
    firstName: 'Updated',
    email: 'ignored@example.test',
  },
  400,
  'EMAIL_CAN_NOT_BE_UPDATED',
);
await expect(owner, '/update-user', {
  name: 'Updated Name',
  firstName: 'Updated',
});
const updated = (await expect(owner, '/get-session')).body.user;
assert.equal(updated.name, 'Updated Name');
assert.equal(updated.firstName, 'Updated');
assert.equal(updated.email, credentials.email);
await expect(
  owner,
  '/change-password',
  {
    currentPassword: 'wrong-test-password',
    newPassword: 'next-test-only-password',
  },
  400,
  'INVALID_PASSWORD',
);
const changed = (
  await expect(owner, '/change-password', {
    currentPassword: credentials.password,
    newPassword: 'next-test-only-password',
    revokeOtherSessions: true,
  })
).body;
assert.ok(changed.token);
assert.equal(changed.user.id, user.id);
assert.equal((await expect(ownerPeer, '/get-session')).body, null);
assert.equal((await expect(owner, '/get-session')).body.user.id, user.id);
assert.equal((await expect(owner, '/list-sessions')).body.length, 1);
await expect(
  ownerPeer,
  '/sign-in/email',
  { email: credentials.email, password: credentials.password },
  401,
  'INVALID_EMAIL_OR_PASSWORD',
);
await expect(ownerPeer, '/sign-in/email', {
  email: credentials.email,
  password: 'next-test-only-password',
});
const enabled = await betterAuthReference(root, { google: true });
assert.equal(enabled.config.socialProviders.google.enabled, true);
assert.deepEqual((await enabled.discovery()).body.socialProviders, ['google']);
const oauthClient = enabled.client();
const oauth = await expect(oauthClient, '/sign-in/social', {
  provider: 'google',
  callbackURL: `${enabled.config.baseURL}/complete`,
});
const authorization = new URL(oauth.body.url);
assert.equal(authorization.origin, 'https://accounts.google.com');
assert.equal(authorization.searchParams.get('client_id'), 'fixture-google-id');
assert.equal(
  authorization.searchParams.get('redirect_uri'),
  `${enabled.config.baseURL}/api/auth/callback/google`,
);
assert.equal(authorization.searchParams.get('response_type'), 'code');
assert.equal(authorization.searchParams.get('code_challenge_method'), 'S256');
assert.ok(authorization.searchParams.get('state'));
assert.ok(authorization.searchParams.get('code_challenge'));
assert.ok(authorization.searchParams.get('scope').includes('openid'));
await expect(
  enabled.client(),
  '/sign-in/social',
  { provider: 'google', callbackURL: 'http://evil.example.test/complete' },
  403,
  'INVALID_CALLBACK_URL',
);
console.log(
  `PASS actual configured Better Auth 1.7.6: 70 HTTP route descriptors, ${count} protocol checks, signed/revoked cookies, credentials, EdDSA/JWKS, organization defaults, configured permission denials, disabled flows and discovery; Rust protocol parity remains open`,
);
