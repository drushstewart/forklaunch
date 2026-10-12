import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import vm from 'node:vm';
import { actualValidator } from '../rust_actual_validator_fixture.mjs';
export async function betterAuthServer(root, reference, options = {}) {
  const actual = actualValidator(root);
  const { trustedCallbackUrl, scriptJson } = actual.load(
    path.join(
      root,
      'blueprint/iam-better-auth/domain/utils/betterAuthRedirect.util.ts'
    )
  );
  const routes = new Map();
  let fetches = 0;
  const app = {
    internal: {
      get(route, handler) {
        routes.set(route, handler);
      },
      all() {}
    },
    use() {},
    registerSdks() {},
    listen(_port, _host, callback) {
      callback();
    }
  };
  const log = { info() {} };
  const tokens = new Proxy({}, { get: (_, key) => key });
  const values = {
    BetterAuth: reference.auth,
    HOST: 'localhost',
    PORT: 3000,
    VERSION: '1',
    DOCS_PATH: '/docs',
    OtelCollector: log,
    Orm: {},
    ExpressApplicationOptions: {}
  };
  const source = await readFile(
    path.join(root, 'blueprint/iam-better-auth/server.ts'),
    'utf8'
  );
  const compiled = actual.ts.transpileModule(source, {
    compilerOptions: {
      module: actual.ts.ModuleKind.CommonJS,
      target: actual.ts.ScriptTarget.ES2022
    }
  }).outputText;
  await vm.runInNewContext(`(async()=>{${compiled}\n})()`, {
    exports: {},
    URL,
    URLSearchParams,
    async fetch(url, requestOptions) {
      fetches++;
      if (options.fetch) return options.fetch(url, requestOptions);
      assert.equal(
        new URL(url).origin,
        new URL(reference.config.baseURL).origin
      );
      return reference.auth.handler(new Request(url, requestOptions));
    },
    require(specifier) {
      if (specifier === '@forklaunch/blueprint-core')
        return {
          forklaunchExpress: () => app,
          schemaValidator: actual.validator
        };
      if (specifier === '@forklaunch/core/persistence')
        return { setupRls() {}, setupTenantFilter() {} };
      if (specifier.endsWith('/bootstrapper'))
        return { tokens, ci: { resolve: (token) => values[token] } };
      if (specifier.endsWith('/sdk')) return { iamSdkClient: {} };
      if (specifier.includes('/api/routes/')) return {};
      if (specifier.endsWith('betterAuth.middleware'))
        return {
          betterAuthTelemetryHookMiddleware() {},
          enrichBetterAuthApi: () => () => {}
        };
      if (specifier.endsWith('betterAuthRedirect.util'))
        return { trustedCallbackUrl, scriptJson };
      throw new Error(`Unexpected server dependency: ${specifier}`);
    }
  });
  return {
    routes,
    fetches: () => fetches,
    async call(route, query, cookie = '') {
      const result = { status: 200, headers: {} };
      const response = {
        status(status) {
          result.status = status;
          return this;
        },
        send(body) {
          result.body = body;
          return this;
        },
        type(type) {
          result.type = type;
          return this;
        },
        setHeader(name, value) {
          result.headers[name.toLowerCase()] = value;
        },
        redirect(url) {
          result.status = 302;
          result.location = url;
        }
      };
      // Host headers must not choose the token-exchange authority.
      await routes.get(route)(
        {
          query,
          headers: { host: 'evil.example.test', cookie },
          protocol: 'http'
        },
        response
      );
      options.onCall?.();
      return result;
    }
  };
}
