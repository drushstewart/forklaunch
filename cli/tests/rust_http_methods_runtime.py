#!/usr/bin/env python3
"""All HTTP verbs through actual module hooks, two SDK sources and lifecycle edits."""
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
env = dict(os.environ)
cli = Path(env.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
root = Path(tempfile.mkdtemp(prefix='forklaunch-methods-')).resolve()
app = root / 'original'
methods = ['Get', 'Post', 'Put', 'Patch', 'Delete', 'Head', 'Options', 'Trace',
           'Checkout', 'Copy', 'Link', 'Lock', 'MSearch', 'Merge', 'MkActivity', 'MkCol',
           'Move', 'Notify', 'PropFind', 'PropPatch', 'Purge', 'Report', 'Search',
           'Subscribe', 'Unlink', 'Unlock', 'Unsubscribe']
print('CLI SHA256:', hashlib.sha256(cli.read_bytes()).hexdigest(), flush=True)


def run(*arguments, cwd=None):
    print('Running:', shlex.join(map(str, arguments)), flush=True)
    subprocess.run(list(map(str, arguments)), cwd=cwd or app, env=env, check=True, timeout=1800)


def command(*arguments, path=None, success=True):
    args = [str(cli), *arguments, '--path', str(path or app)]
    print('Running:', shlex.join(args), flush=True)
    result = subprocess.run(args, cwd=root, env=env, capture_output=True, text=True, timeout=300)
    print(result.stdout, end='', flush=True)
    if success:
        assert result.returncode == 0, result.stderr
    else:
        assert result.returncode != 0, result.stdout
    return result


def snapshot():
    return {str(path.relative_to(app)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in app.rglob('*') if path.is_file()}


def contracts():
    manifest = tomllib.loads((app / '.forklaunch/manifest.toml').read_text())
    return {project['name']: json.loads((app / project['metadata']['contractArtifactPath']).read_text())
            for project in manifest['projects'] if project['name'] in ['iam', 'cac', 'billing']}


def cac_openapi():
    manifest = tomllib.loads((app / '.forklaunch/manifest.toml').read_text())
    project = next(project for project in manifest['projects'] if project['name'] == 'cac')
    return app / project['metadata']['openapiPath']


def retain_aggregate_cac():
    """Retain the former module layout using the complete raw manifest/sources."""
    directory = app / 'src/cac'
    cargo = directory / 'Cargo.toml'
    source = re.sub(r'^contracts\s*=.*\n', '', cargo.read_text(), flags=re.M)
    source = re.sub(r'^\[(?:build-dependencies|dependencies)\.contracts\]\n.*?(?=^\[|\Z)', '', source, flags=re.M | re.S)
    cargo.write_text(source)
    (directory / 'api/contracts/mod.rs').write_text('pub mod registry;\npub mod extensions;\n')
    (directory / 'build.rs').write_text((repo / 'cli/src/templates/rust/project/module-sdk-build.rs').read_text().replace('{{{name}}}', 'cac'))
    shutil.rmtree(directory / '.forklaunch/contracts')
    shutil.rmtree(directory / 'sdk')
    workspace = app / 'Cargo.toml'
    source = workspace.read_text()
    members = [member for member in tomllib.loads(source)['workspace']['members']
               if member not in ['src/cac/.forklaunch/contracts', 'src/cac/sdk/rust']]
    workspace.write_text(re.sub(r'^members\s*=\s*\[.*?\]', 'members = ' + json.dumps(members), source, flags=re.M | re.S))
    manifest = app / '.forklaunch/manifest.toml'
    source = manifest.read_text()
    begin = source.rfind('[[projects]]', 0, source.index('name = "cac"'))
    end = source.find('[[projects]]', begin + 1)
    end = len(source) if end == -1 else end
    section = re.sub(r'^sdkLayout\s*=.*\n', '', source[begin:end], flags=re.M)
    section = re.sub(r'^contractArtifactPath\s*=.*$', 'contractArtifactPath = ".forklaunch/contracts/cac/contract.json"', section, flags=re.M)
    section = re.sub(r'^openapiPath\s*=.*$', 'openapiPath = ".forklaunch/openapi/cac/openapi.json"', section, flags=re.M)
    manifest.write_text(source[:begin] + section + source[end:])
    command('init', 'module', 'cac', '--language', 'rust', '-m', 'cac-base')
    assert not (directory / 'sdk/rust').exists()


def check_local_types(tsc):
    """Call all generated methods through their actual discrete type export."""
    document = json.loads(cac_openapi().read_text())
    descriptors = []
    for path, item in document['paths'].items():
        if path.startswith('/verbs/'):
            entries = [value for key, value in item.items() if key != 'x-forklaunch-http-methods']
            entries += list(item.get('x-forklaunch-http-methods', {}).values())
            descriptors += [entry['x-forklaunch-operation'] for entry in entries if isinstance(entry, dict) and 'x-forklaunch-operation' in entry]
    assert len(descriptors) == 27, descriptors
    source = 'import type { CacSdkClient } from "./src/cac/sdk/typescript/sdk.js";\nexport async function methods(client: CacSdkClient) {\n'
    for index, descriptor in enumerate(descriptors):
        access = ''.join('[' + json.dumps(part) + ']' for part in descriptor['clientPath'])
        source += f'''const response{index} = await client{access}({{}});
if (response{index}.status !== 204) throw new Error('Declared framework error');
const status{index}: 204 = response{index}.status;
const body{index}: undefined = response{index}.response;
// @ts-expect-error the successful bodyless response cannot become a string
const wrongBody{index}: string = response{index}.response;
'''
    source += '// @ts-expect-error an undeclared route is not part of the generated client\nawait client.notARegisteredRoute({});\n}\n'
    fixture = app / 'method-types.ts'
    fixture.write_text(source)
    config = app / 'method-types.tsconfig.json'
    config.write_text(json.dumps({'compilerOptions': {
        'noEmit': True, 'strict': True, 'skipLibCheck': True, 'target': 'ES2022',
        'module': 'ESNext', 'moduleResolution': 'Bundler', 'lib': ['ES2022', 'DOM', 'DOM.Iterable'],
    }, 'files': [str(fixture)]}))
    run(tsc, '--project', config)


try:
    command('init', 'application', 'methods', '--database', 'postgresql', '--validator', 'zod',
            '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express',
            '--runtime', 'node', '--test-framework', 'vitest', '-D', 'Methods', '-A', 'ForkLaunch', '-L', 'MIT')
    before = snapshot()
    command('init', 'router', 'invalid', '--language', 'rust', success=False)
    assert snapshot() == before, 'Rejected app-root router changed the application'
    for name in ['iam', 'cac', 'billing']:
        # The messaging module is retired (email/SMS go through the managed
        # gateway); billing-base is the third builtin router lifecycle.
        command('init', 'module', name, '--language', 'rust', '-m', name + '-base')
        command('init', 'router', 'custom', '--language', 'rust', path=app / 'src' / name)
    # An ordinary service supplies the same public transport package shipped in
    # normal mixed apps. Builtin aggregate SDKs also retain their own facade.
    command('init', 'service', 'transport', '--language', 'rust')
    relocated = root / 'relocated'
    app.rename(relocated)
    app = relocated
    assert not (root / 'original').exists()

    # Builtin modules expose declaration and runtime extension hooks separately.
    # The build host imports the declarations without importing Axum handlers.
    declarations = app / 'src/cac/api/contracts/extensions/custom.rs'
    declarations.write_text('''use forklaunch_core::contract::{RouteContract, HttpMethod};
pub const ROUTE_NAME: &str = "custom";
pub fn contract() -> RouteContract { RouteContract::get("custom", "custom", format!("/{ROUTE_NAME}")).with_response(204, None) }
pub fn all_contracts() -> Vec<RouteContract> { vec![
''' + ''.join(f'RouteContract::new("Probe {method}", "HTTP method", HttpMethod::{method}, "/verbs/{method.lower()}").with_response(204, None),\n' for method in methods) + '''] }
pub fn contracts() -> Vec<RouteContract> { let mut routes = all_contracts(); routes.push(contract()); routes }
''')
    handler = app / 'src/cac/api/routes/extensions/custom.rs'
    handler.write_text('''use forklaunch_axum::ForkLaunchRouter;
pub use crate::api::contracts::extensions::custom::{contract, contracts, ROUTE_NAME};
use crate::api::contracts::extensions::custom::all_contracts;
pub fn register(mut router: ForkLaunchRouter) -> ForkLaunchRouter {
    router = router.get(contract(), || async { axum::http::StatusCode::NO_CONTENT });
    for contract in all_contracts() {
        let method = contract.method;
        router = router.route(contract, move |request: axum::extract::Request| async move {
            assert_eq!(request.method().as_str(), method.as_str());
            axum::http::StatusCode::NO_CONTENT
        });
    }
    router
}
''')
    run('cargo', 'build', '--workspace')
    command('openapi', 'export')
    runtime = root / 'typescript-runtime'
    import sys; sys.path.insert(0, str(repo / 'cli/tests/fixtures')); from universal_sdk_source import copy_universal_sdk
    copy_universal_sdk(repo, runtime)
    run('npm', 'install', '--prefix', runtime, '--ignore-scripts', '--package-lock=false')
    run('npm', '--prefix', runtime, 'run', 'build')
    tsc = Path(env.get('TSC_BIN', runtime / 'node_modules/.bin/tsc'))
    assert tsc.is_file(), tsc
    env['TSC_BIN'] = str(tsc)
    run('node', repo / 'cli/tests/rust_http_methods_parity.mjs',
        app / 'src/cac/sdk/typescript/contract.json')
    run('pnpm', 'install', '--ignore-scripts')
    run('pnpm', 'run', 'sdk:typecheck')
    sys.path.insert(0, str(repo / 'cli/tests/fixtures'))
    from rust_builtin_runtime_support import rust_consumer

    for layout in ['local', 'aggregate']:
        if layout == 'aggregate':
            retain_aggregate_cac()
            run('cargo', 'build', '--workspace')
            command('openapi', 'export')
        consumer = root / ('rust-consumer-' + layout)
        consumer_manifest = rust_consumer(app, consumer, 'cac', package='methods-sdk-consumer')
        (consumer / 'tests').mkdir(exist_ok=True)
        for source in ['contract', 'openapi']:
            command('sdk', 'generate', '--target', 'all', '--from', source)
            if layout == 'local':
                generated = app / 'src/cac/sdk/rust/generated'
                check_local_types(tsc)
                sdk_entry = '-'
                imports = 'use cac_sdk::*;\n'
                client = 'Client'
            else:
                sdk = app / 'src/client-sdk/typescript'
                run('npm', 'install', '--prefix', sdk, '--ignore-scripts', '--package-lock=false')
                config = app / 'method-aggregate.tsconfig.json'
                config.write_text(json.dumps({'compilerOptions': {
                    'noEmit': True, 'strict': True, 'skipLibCheck': True, 'target': 'ES2022',
                    'module': 'ESNext', 'moduleResolution': 'Bundler', 'lib': ['ES2022', 'DOM', 'DOM.Iterable'],
                }, 'files': [str(sdk / 'index.ts')]}))
                run(tsc, '--project', config)
                # The retained package declares TypeScript source exports with
                # bundler resolution. Bundle those actual exports for Node; raw
                # tsc output preserves their extensionless internal imports.
                run(runtime / 'node_modules/.bin/esbuild', sdk / 'index.ts',
                    '--bundle', '--platform=node', '--format=esm', '--packages=external',
                    '--outfile=' + str(sdk / 'dist/index.js'))
                generated = app / 'src/client-sdk/rust/cac'
                sdk_entry = sdk / 'dist/index.js'
                imports = 'use methods_sdk_consumer::*;\n'
                client = 'ForkLaunchClient'
            run('cargo', 'check', '--workspace', '--all-targets')
            operations = re.findall(r'pub async fn (probe_\w+)\(\s*&self,\s*(?:input|request): (\w+)', (generated / 'client.rs').read_text())
            assert len(operations) == 27, operations
            test = imports + f'#[tokio::test]\nasync fn all_methods() {{ let client = {client}::new(std::env::var("METHODS_BASE_URL").unwrap());\n'
            test += ''.join(f'assert!(matches!(client.{method}({request}::default()).await.unwrap(), {request.removesuffix("Request")}Responses::Status204(())));\n' for method, request in operations)
            (consumer / 'tests/methods.rs').write_text(test + '}\n')
            run('node', repo / 'cli/tests/rust_http_methods_runtime.mjs', Path(env['CARGO_TARGET_DIR']) / 'debug/cac',
                sdk_entry, consumer_manifest, runtime / 'lib/index.mjs', cac_openapi())
            print('PASS all27 methods through ' + layout + ' SDK layout and ' + source + ' source', flush=True)

    # Both authored handler and declaration bodies survive change/sync. The
    # changed marker proves lifecycle operations retained the extension hooks.
    for name in ['iam', 'cac', 'billing']:
        command('change', 'router', '-e', 'custom', '-N', 'extra', '--confirm', path=app / 'src' / name)
        command('sync', 'service', name)
    assert 'request.method().as_str()' in (app / 'src/cac/api/routes/extensions/extra.rs').read_text()
    run('cargo', 'check', '--workspace', '--all-targets')
    command('openapi', 'export')
    for name, artifact in contracts().items():
        assert any(route['path'] == '/extra' for route in artifact['routes']), name
        assert not any(route['path'] == '/custom' for route in artifact['routes']), name
    assert len([route for route in contracts()['cac']['routes'] if route['path'].startswith('/verbs/')]) == 27
    for name in ['iam', 'cac', 'billing']:
        command('delete', 'router', 'extra', '--continue', path=app / 'src' / name)
    run('cargo', 'check', '--workspace', '--all-targets')
    command('openapi', 'export')
    for name, artifact in contracts().items():
        assert not any(route['path'] == '/extra' for route in artifact['routes']), name
    assert not any(route['path'].startswith('/verbs/') for route in contracts()['cac']['routes'])
    server = app / 'src/billing/api/routes/mod.rs'
    original = server.read_text()
    assert 'extensions::register_router(' in original
    server.write_text(original.replace('extensions::register_router(', '('))
    before = snapshot()
    result = command('init', 'router', 'unsupported', '--language', 'rust', path=app / 'src/billing', success=False)
    assert 'does not expose custom router hooks' in result.stdout + result.stderr
    assert snapshot() == before, 'Missing-hook rejection changed application files'
    print('PASS all 27 HTTP methods, both SDK sources and all builtin module router lifecycles', flush=True)
finally:
    if env.get('FORKLAUNCH_KEEP_FIXTURE'):
        print('Methods fixture evidence:', root, flush=True)
    else:
        shutil.rmtree(root)
