// Compare actual configured TS controllers/routes and validator schemas.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { actualValidator } from './rust_actual_validator_fixture.mjs';
const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..'
);
const captured = [];
const handlers = new Proxy(
  {},
  {
    get: (_, method) => (_validator, _path, options) => ({ method, options })
  }
);
const router = (prefix) =>
  new Proxy(
    {},
    {
      get: (_, method) => (suffix, handler) => {
        assert.equal(method, handler.method);
        captured.push({
          method: method.toUpperCase(),
          path: prefix + suffix,
          options: handler.options
        });
        return handler;
      }
    }
  );
const tokens = new Proxy({}, { get: (_, name) => name });
const ci = { resolve: () => ({}), scopedResolver: () => () => ({}) };
let source;
source = actualValidator(root, (specifier) => {
  if (specifier === '@forklaunch/blueprint-core')
    return {
      ...source.statics,
      schemaValidator: source.validator,
      handlers,
      forklaunchRouter: router,
      IdSchema: { id: source.statics.string },
      PLATFORM_SYSTEM_ROLES: new Set(['system'])
    };
  if (specifier.endsWith('/bootstrapper')) return { ci, tokens };
  return undefined;
});
for (const family of ['discovery', 'user', 'compliance']) {
  source.load(
    path.join(root, `blueprint/iam-better-auth/api/routes/${family}.routes.ts`)
  );
}
const { toPrettyCamelCase } = source.load(
  path.join(root, 'framework/common/src/camelCase.ts')
);
const native = JSON.parse(
  execFileSync(
    'cargo',
    [
      'run',
      '--quiet',
      '--manifest-path',
      'blueprint-rs/Cargo.toml',
      '-p',
      'forklaunch-iam-better-auth',
      '--features',
      'application-http',
      '--example',
      'application_contract'
    ],
    {
      cwd: root,
      encoding: 'utf8',
      env: { ...process.env, CARGO_INCREMENTAL: '0' },
      timeout: 120000
    }
  )
);
assert.equal(native.routes.length, captured.length);
function shape(schema) {
  if (Array.isArray(schema)) return schema.map(shape);
  if (!schema || typeof schema !== 'object') return schema;
  return Object.fromEntries(
    Object.entries(schema)
      .filter(
        ([key, value]) =>
          ![
            'additionalProperties',
            'description',
            'example',
            'title',
            '$schema'
          ].includes(key) && !(key === 'format' && value === 'double')
      )
      .map(([key, value]) => [
        key,
        key === 'required'
          ? [...value].sort()
          : key === 'type' && Array.isArray(value) && value.length === 1
            ? value[0]
            : shape(value)
      ])
  );
}
let checks = 0;
for (const route of captured) {
  const actual = native.routes.find(
    (value) => value.operationName === toPrettyCamelCase(route.options.name)
  );
  assert.ok(actual, route.options.name);
  assert.equal(actual.name, route.options.name);
  assert.equal(actual.method, route.method);
  assert.equal(actual.path, route.path);
  assert.equal(actual.access, route.options.access);
  assert.equal(actual.summary, route.options.summary);
  assert.deepEqual(
    actual.request.params.map((field) => field.name).sort(),
    Object.keys(route.options.params ?? {}).sort()
  );
  assert.deepEqual(
    actual.responses.map((response) => response.statusCode).sort(),
    Object.keys(route.options.responses).map(Number).sort()
  );
  if (route.options.auth) {
    assert.equal(actual.auth.method, route.options.auth.hmac ? 'hmac' : 'jwt');
    if (route.options.auth.allowedRoles)
      assert.deepEqual(actual.rbac.allowedRoles, [
        ...route.options.auth.allowedRoles
      ]);
  }
  for (const response of actual.responses) {
    assert.match(response.body.schema.name, /^[a-zA-Z0-9._-]+$/,
      `${route.options.name}: response component name must be valid in OpenAPI`);
    const schema = source.validator.openapi(
      route.options.responses[response.statusCode]
    );
    assert.deepEqual(
      shape(response.body.schema.jsonSchema),
      shape(schema),
      `${route.options.name} ${response.statusCode}`
    );
    checks++;
  }
}
console.log(
  `PASS ${captured.length} actual Better Auth application contracts and ${checks} response schemas`
);
