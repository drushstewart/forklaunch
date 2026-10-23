#!/usr/bin/env python3
"""Local clients from raw scaffolds plus the manual TS OpenAPI export boundary."""
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
env = {**os.environ, 'CARGO_INCREMENTAL': '0', 'RUSTUP_TOOLCHAIN': 'stable',
       'FORKLAUNCH_SKIP_FORMAT': '1'}
env.setdefault('CARGO_TARGET_DIR', str(repo / 'cli/target'))
if not env.get('FORKLAUNCH_WORKER_RUNTIME_LOCKED'):
    sys.exit(subprocess.run([sys.executable, str(repo / 'cli/tests/fixtures/worker_runtime_lock.py'),
        '-c', shlex.join([sys.executable, str(Path(__file__).resolve())])], env=env).returncode)
print('CLI SHA256:', hashlib.sha256(binary.read_bytes()).hexdigest(), flush=True)
output = repo / 'cli/tests/output/rust-mixed-sdk-runtime'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
app = root / 'app'
children = []


def run(*args, cwd=None, timeout=1800):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run([str(arg) for arg in args], cwd=app if cwd is None else cwd, env=env, check=True, timeout=timeout)


def cli(*args):
    run(binary, *args, '--path', app, cwd=root)


def project_metadata(name, **updates):
    """Edit only this registered project's metadata; retain the full manifest."""
    manifest = app / '.forklaunch/manifest.toml'
    parts = manifest.read_text().split('[[projects]]')
    for index, part in enumerate(parts[1:], 1):
        if re.search(r'^name = ' + re.escape(json.dumps(name)) + r'$', part, re.M):
            assert '[projects.metadata]' in part
            for key, value in updates.items():
                pattern = r'^' + re.escape(key) + r' = .*\n'
                part = re.sub(pattern, '', part, flags=re.M)
                if value is not None:
                    part = part.replace('[projects.metadata]\n', '[projects.metadata]\n' + key + ' = ' + json.dumps(value) + '\n', 1)
            parts[index] = part
            manifest.write_text('[[projects]]'.join(parts))
            parsed = tomllib.loads(manifest.read_text())
            assert next(project for project in parsed['projects'] if project['name'] == name)['metadata']['sdkLayout'] == 'project-local-v1'
            return
    raise AssertionError('Project not registered: ' + name)


def start(*args):
    child = subprocess.Popen([str(arg) for arg in args], cwd=app,
        env={**{key: value for key, value in env.items() if not key.startswith(('MCP_', 'OTEL_'))},
             'HOST': '127.0.0.1', 'PORT': '0', 'PROTOCOL': 'http', 'VERSION': 'v1',
             'DOCS_PATH': '/docs', 'RUST_LOG': 'info', 'MCP_ENABLED': 'true'},
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    children.append(child)
    deadline = time.monotonic() + 20
    lines = []
    while time.monotonic() < deadline:
        if select.select([child.stdout], [], [], .1)[0]:
            line = child.stdout.readline()
            lines.append(line)
            match = re.search(r'listening on (http://127\.0\.0\.1:\d+)', line)
            if match:
                return match[1]
        assert child.poll() is None, ''.join(lines)
    raise AssertionError('Service startup timed out: ' + ''.join(lines))


try:
    cli('init', 'application', 'mixed-sdk', '--database', 'postgresql', '--validator', 'zod',
        '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express',
        '--runtime', 'node', '--test-framework', 'vitest', '-D', 'Mixed SDK', '-A', 'ForkLaunch', '-L', 'MIT')
    cli('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
    refused = subprocess.run([str(binary), 'sdk', 'mode', '-t', 'live', '--path', str(app)], cwd=root, env=env, capture_output=True, text=True, timeout=30)
    assert refused.returncode != 0 and 'requires a TypeScript project' in refused.stderr + refused.stdout
    tools = root / 'tools'
    tools.mkdir()
    (tools / 'package.json').write_text('{"private":true}')
    run('npm', 'install', '--ignore-scripts', '--no-audit', '--no-fund', 'tsx@4.20.5',
        'typescript@5.9.3', '@types/node@22', cwd=tools)
    cli('init', 'service', 'typescript-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Manual export hook')
    ts = app / 'src/typescript-api'
    # A manually owned export hook produces OpenAPI directly; contract mode
    # intentionally imports that document rather than a managed build artifact.
    project_metadata('typescript-api', contractArtifactPath=None)
    (ts / 'sdk/contract.json').unlink(missing_ok=True)
    (ts / 'node_modules').symlink_to(tools / 'node_modules', target_is_directory=True)
    package = ts / 'package.json'
    package_data = json.loads(package.read_text())
    package_data.update({'name': 'typescript-api', 'private': True, 'type': 'module'})
    # This fixture deliberately owns a plain TypeScript server/export hook.
    # Keep its normal build contract so later mixed lifecycle reconciliation
    # can still construct the shared TypeScript image.
    package_data.setdefault('scripts', {})['build'] = 'tsc -b'
    package.write_text(json.dumps(package_data))
    (ts / 'tsconfig.json').write_text(json.dumps({'compilerOptions': {
        'target': 'ES2022', 'module': 'NodeNext', 'moduleResolution': 'NodeNext',
        'strict': True, 'skipLibCheck': True, 'rootDir': '.', 'outDir': 'lib',
    }, 'include': ['server.ts']}))
    profile = {'type': 'object', 'properties': {'id': {'type': 'string'}, 'profileName': {'type': 'string'}}, 'required': ['id', 'profileName'], 'additionalProperties': False}
    def response(schema):
        return {'description': 'Fixture response', 'content': {'application/json': {'schema': schema}}}
    spec = {'openapi': '3.1.0', 'info': {'title': 'TypeScript profiles', 'version': '1'}, 'paths': {
        '/profiles/{id}': {'get': {'operationId': 'getProfile', 'parameters': [
            {'name': 'id', 'in': 'path', 'required': True, 'schema': {'type': 'string'}}],
            'responses': {'200': response({'$ref': '#/components/schemas/Profile'}),
                          '404': response({'type': 'object', 'properties': {'errorCode': {'type': 'string'}}, 'required': ['errorCode'], 'additionalProperties': False})}}},
        '/profiles': {'post': {'operationId': 'createProfile', 'requestBody': {'required': True, 'content': {
            'application/json': {'schema': {'type': 'object', 'properties': {'profileName': {'type': 'string'}}, 'required': ['profileName'], 'additionalProperties': False}}}},
            'responses': {'201': response({'$ref': '#/components/schemas/Profile'})}}}},
        'components': {'schemas': {'Profile': profile}}}
    (ts / 'spec.json').write_text(json.dumps(spec))
    (ts / 'server.ts').write_text('''import {readFileSync,writeFileSync} from 'node:fs';
import {createServer} from 'node:http';
if(process.env.FORKLAUNCH_MODE==='openapi') {
  writeFileSync(process.env.FORKLAUNCH_OPENAPI_OUTPUT!,readFileSync(new URL('./spec.json',import.meta.url)));
} else {
  const server=createServer(async(req,res)=>{
    res.setHeader('content-type','application/json');
    if(!req.headers['x-sdk-consumer']) {res.writeHead(400);res.end('{}');return;}
    const path=new URL(req.url!,'http://fixture').pathname;
    if(req.method==='GET'&&path.startsWith('/profiles/')) {
      const id=decodeURIComponent(path.slice('/profiles/'.length));
      if(id==='missing'){res.writeHead(404);res.end(JSON.stringify({errorCode:'not_found'}));}
      else {res.end(JSON.stringify({id,profileName:'Ada'}));}
    } else if(req.method==='POST'&&path==='/profiles') {
      let raw='';for await(const chunk of req)raw+=chunk;
      res.writeHead(201);res.end(JSON.stringify({id:'created',profileName:JSON.parse(raw).profileName}));
    } else {res.writeHead(404);res.end('{}');}
  });
  server.listen(0,'127.0.0.1',()=>console.log(`listening on http://127.0.0.1:${(server.address() as import('node:net').AddressInfo).port}`));
  process.on('SIGTERM',()=>server.close(()=>process.exit(0)));
}
''')
    # TS declarations are application-owned, just as in a normal TS service.
    # The schema compiler owns its Rust SDK and the native service's TS SDK.
    authored_types = """import type { ForkLaunchSdkRequest, ForkLaunchSdkResponse } from '@forklaunch/universal-sdk';
export type Profile = { id: string; profileName: string };
export type GetProfileRequest = ForkLaunchSdkRequest<{ id: string }>;
export type CreateProfileRequest = ForkLaunchSdkRequest<never, never, { profileName: string }>;
export type TypescriptApiSdkClient = {
  profiles: {
    getProfile(input: GetProfileRequest): Promise<ForkLaunchSdkResponse<{ 200: Profile; 404: { errorCode: string } }, never, Record<number, 'application/json'>>>;
    createProfile(input: CreateProfileRequest): Promise<ForkLaunchSdkResponse<{ 201: Profile }, never, Record<number, 'application/json'>>>;
  };
};
"""
    (ts / 'sdk/index.ts').write_text(authored_types)
    cli('openapi', 'export')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    # Lifecycle must retain a previously exported TypeScript client.
    cli('sync', 'service', 'native-api')
    assert (ts / 'sdk/index.ts').read_text() == authored_types
    assert (ts / 'sdk/rust/lib.rs').is_file()
    assert (app / 'src/native-api/sdk/typescript/index.ts').read_text().strip() == "export type * from './sdk.js';"
    cli('sdk', 'generate', '--target', 'all', '--from', 'openapi')
    # Source tracking and generated packages must survive relocation.
    relocated = root / 'relocated-app'
    app.rename(relocated)
    app = relocated
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    # Mixed live mode resolves TS source directly while preserving Rust artifacts
    # and shared orchestration; generated mode restores declaration resolution.
    ts = app / 'src/typescript-api'
    package = ts / 'package.json'
    package_data = json.loads(package.read_text())
    package_data['main'] = 'index.ts'
    package.write_text(json.dumps(package_data))
    (ts / 'index.ts').write_text("export type LiveProfile = { source: 'live' };\n")
    (ts / 'index.d.ts').write_text("export type LiveProfile = { source: 'generated' };\n")
    (tools / 'node_modules/typescript-api').symlink_to(ts, target_is_directory=True)
    (app / 'node_modules').symlink_to(tools / 'node_modules', target_is_directory=True)
    (app / 'tsconfig.json').write_text('{"compilerOptions":{"strict":true},"references":[]}')
    (app / '.vscode').mkdir(exist_ok=True)
    (app / '.vscode/tasks.json').write_text('{"version":"2.0.0","tasks":[{"label":"Owned Cargo task","command":"cargo check","type":"shell"}]}')
    native_package = app / 'src/native-api/package.json'
    native_package.parent.mkdir(parents=True, exist_ok=True)
    native_package.write_text('{"types":"native-sentinel.d.ts"}')
    protected_paths = [app/'tsconfig.json',app/'.vscode/tasks.json',app/'Cargo.toml',native_package,app/'src/native-api/sdk/typescript/index.ts',ts/'sdk/index.ts']
    protected = {path:path.read_bytes() for path in protected_paths}
    def check_live_type(marker):
        consumer = app/'live-import.mts'
        consumer.write_text("import type {LiveProfile} from 'typescript-api';\nconst value:LiveProfile={source:"+json.dumps(marker)+"}; void value;\n")
        run(tools/'node_modules/.bin/tsc','--noEmit','--strict','--skipLibCheck','--target','ES2022','--module','NodeNext','--moduleResolution','NodeNext','live-import.mts')
    cli('sdk','mode','-t','generated')
    assert json.loads(package.read_text())['types']=='index.d.ts'
    check_live_type('generated')
    original_package = package.read_bytes()
    cli('sdk','mode','-t','live','--dryrun')
    assert package.read_bytes()==original_package
    cli('sdk','mode','-t','live')
    assert json.loads(package.read_text()).get('types') is None
    check_live_type('live')
    (ts/'index.ts').write_text("export type LiveProfile = { source: 'updated' };\n")
    check_live_type('updated')
    cli('sdk','mode','-t','generated')
    check_live_type('generated')
    cli('sdk','mode','-t','live')
    check_live_type('updated')
    for path,content in protected.items():
        assert path.read_bytes()==content, path
    print('PASS mixed live TS imports follow source edits, generated mode restores declarations, dry-run and native artifacts are preserved',flush=True)
    # Watch both the manifest and a relocated TS export even in contract mode.
    sdk_source = ts / 'sdk/rust/models.rs'
    manifest = app / '.forklaunch/manifest.toml'
    artifact = ts / 'sdk/openapi.json'
    custom = app / 'artifacts/typescript/spec.json'
    custom.parent.mkdir(parents=True)
    artifact.rename(custom)
    project_metadata('typescript-api', openapiPath='artifacts/typescript/spec.json')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract')
    cli('init', 'service', 'disposable-api', '--language', 'rust', '-D', 'Lifecycle fixture')
    # A real local-SDK consumer exercises aggregate reconciliation on deletion.
    # An app with only project-local SDKs can delete an unrelated service without
    # reading any other project's temporarily missing export.
    cli('init', 'library', 'clients', '--language', 'rust', '--sdk-services', 'native-api')
    aggregate = app / 'src/client-sdk/rust'
    # The aggregate re-exports every service-local SDK (e3842817d); the one
    # named on --sdk-services and the newly added native service are among them.
    aggregate_dependencies = tomllib.loads((aggregate / 'Cargo.toml').read_text())['dependencies']
    assert aggregate_dependencies['native_api'] == {'package': 'native-api-sdk', 'path': '../../native-api/sdk/rust'}, aggregate_dependencies
    assert aggregate_dependencies['disposable_api'] == {'package': 'disposable-api-sdk', 'path': '../../disposable-api/sdk/rust'}, aggregate_dependencies
    # Compilation publishes the newly added service's contract before the
    # watcher validates all registered services.
    run('cargo', 'build', '-p', 'disposable-api', '-p', 'clients')
    watch_log = root / 'watch.log'
    with watch_log.open('w') as log:
        watcher = subprocess.Popen([str(binary), 'sdk', 'watch', '--path', str(app),
            '--target', 'all', '--from', 'contract', '--interval-ms', '100'], cwd=app, env=env, stdout=log, stderr=log)
        def wait_for(condition):
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                assert watcher.poll() is None, watch_log.read_text()
                if condition():
                    return
                time.sleep(.05)
            raise AssertionError('SDK watch deadline: ' + watch_log.read_text())
        try:
            wait_for(lambda: 'Watching contract' in watch_log.read_text())
            changed = json.loads(custom.read_text())
            changed['components']['schemas']['Profile']['properties']['watchMarker'] = {'type': 'string'}
            completed = watch_log.read_text().count('Generated Rust SDK sources')
            custom.write_text(json.dumps(changed))
            wait_for(lambda: 'watch_marker' in sdk_source.read_text()
                     and watch_log.read_text().count('Generated Rust SDK sources') > completed)
            original = sdk_source.read_bytes()
            # Watch passes wait for a missing TypeScript export instead of
            # failing; the other services still regenerate and the waiting
            # service keeps its existing outputs.
            waiting = 'Waiting for typescript-api to export its OpenAPI document'
            waited = watch_log.read_text().count(waiting)
            completed = watch_log.read_text().count('Generated Rust SDK sources')
            custom.unlink()
            wait_for(lambda: watch_log.read_text().count(waiting) > waited
                     and watch_log.read_text().count('Generated Rust SDK sources') > completed)
            assert 'SDK regeneration failed' not in watch_log.read_text(), watch_log.read_text()
            assert sdk_source.read_bytes() == original
            # Deleting a disposable native service refreshes the existing SDKs.
            failed = subprocess.run([str(binary), 'delete', 'service', 'disposable-api', '--continue', '--path', str(app)],
                cwd=app, env=env, capture_output=True, text=True, timeout=60)
            assert failed.returncode != 0 and 'Restore the TypeScript OpenAPI export' in failed.stderr + failed.stdout, failed.stdout + failed.stderr
            assert sdk_source.read_bytes() == original
            changed['components']['schemas']['Profile']['properties']['recoveredMarker'] = {'type': 'boolean'}
            completed = watch_log.read_text().count('Generated Rust SDK sources')
            custom.write_text(json.dumps(changed))
            wait_for(lambda: 'recovered_marker' in sdk_source.read_text()
                     and watch_log.read_text().count('Generated Rust SDK sources') > completed)
        finally:
            watcher.terminate()
            try:
                watcher.wait(timeout=10)
            except subprocess.TimeoutExpired:
                watcher.kill()
                watcher.wait(timeout=5)
                raise AssertionError('SDK watch did not terminate')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    assert (ts / 'sdk/index.ts').read_text() == authored_types
    print('PASS mixed SDK watch follows custom TS exports, preserves outputs on loss and recovers', flush=True)
    probe = app / 'mixed-probe'
    (probe / 'src').mkdir(parents=True)
    (probe / 'Cargo.toml').write_text('''[package]
name = "mixed-sdk-probe"
version = "0.0.0"
edition = "2024"
[dependencies]
native-api-sdk = { path = "../src/native-api/sdk/rust" }
typescript-api-sdk = { path = "../src/typescript-api/sdk/rust" }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
''')
    (probe / 'src/main.rs').write_text((repo / 'cli/tests/fixtures/rust_mixed_sdk.rs').read_text())
    workspace = app / 'Cargo.toml'
    workspace.write_text(workspace.read_text().replace('members = [', 'members = ["mixed-probe", '))
    run('cargo', 'build', '-p', 'native-api', '-p', 'mixed-sdk-probe', '-p', 'clients')
    run('cargo', 'clippy', '-p', 'native-api-sdk', '-p', 'typescript-api-sdk', '-p', 'mixed-sdk-probe', '--', '-D', 'warnings')
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=app, env=env))
    native = Path(metadata['target_directory']) / 'debug/native-api'
    run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', native)
    native_url = start(native)
    ts_url = start(tools / 'node_modules/.bin/tsx', app / 'src/typescript-api/server.ts')
    runtime = root / 'typescript-runtime'
    import sys; sys.path.insert(0, str(repo / 'cli/tests/fixtures')); from universal_sdk_source import copy_universal_sdk
    copy_universal_sdk(repo, runtime)
    run('npm', 'install', '--ignore-scripts', '--package-lock=false', cwd=runtime)
    run('npm', 'run', 'build', cwd=runtime)
    scope = tools / 'node_modules/@forklaunch'
    scope.mkdir(exist_ok=True)
    (scope / 'universal-sdk').symlink_to(runtime, target_is_directory=True)
    (app / 'mixed-client.mts').write_text('''import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {universalSdk, type UniversalSdkOptions} from '@forklaunch/universal-sdk';
import type {NativeApiSdkClient} from './src/native-api/sdk/typescript/index.js';
import type {TypescriptApiSdkClient} from './src/typescript-api/sdk/index.js';
const seen:string[]=[];
const options: Omit<UniversalSdkOptions, 'registryOptions'> = {
 host:'http://127.0.0.1:1',
 serviceBaseUrls:{'native-api':process.argv[2]!,'typescript-api':process.argv[3]!},
 beforeRequest(request){seen.push(request.service);return {...request,input:{...request.input,headers:{'x-sdk-consumer':'typescript'}}};}
};
const sdk = {
 nativeApi:await universalSdk<NativeApiSdkClient>({...options,registryOptions:{raw:JSON.parse(readFileSync('src/native-api/sdk/typescript/openapi.json','utf8'))}}),
 typescriptApi:await universalSdk<TypescriptApiSdkClient>({...options,registryOptions:{raw:JSON.parse(readFileSync('artifacts/typescript/spec.json','utf8'))}})
};
const health=await sdk.nativeApi.health.getHealth();
assert.equal(health.status,200);if(health.status===200)assert.equal(health.response.status,'ok');
const id='Ada / 🦀 & backend';
const profile=await sdk.typescriptApi.profiles.getProfile({params:{id}});
assert.equal(profile.status,200);if(profile.status===200){assert.equal(profile.response.id,id);assert.equal(profile.response.profileName,'Ada');}
const missing=await sdk.typescriptApi.profiles.getProfile({params:{id:'missing'}});
assert.equal(missing.status,404);if(missing.status===404)assert.equal(missing.response.errorCode,'not_found');
const created=await sdk.typescriptApi.profiles.createProfile({body:{profileName:'Grace'}});
assert.equal(created.status,201);if(created.status===201)assert.equal(created.response.profileName,'Grace');
assert.deepEqual(seen,['native-api','typescript-api','typescript-api','typescript-api']);
if(false){
 // @ts-expect-error path parameters are required
 sdk.typescriptApi.profiles.getProfile();
 // @ts-expect-error the request body is strongly typed
 sdk.typescriptApi.profiles.createProfile({body:{profileName:42}});
 if(profile.status===200){
  // @ts-expect-error undeclared response field
  profile.response.undeclared;
 }
 if(health.status===200){
  // @ts-expect-error native schema field is also strongly typed
  const wrong:string=health.response.recordCount; void wrong;
 }
}
console.log('PASS public TypeScript SDK, live Rust/TypeScript requests, escaped params, typed errors/bodies and service routing/hooks');
''')
    run(tools / 'node_modules/.bin/tsc', '--strict', '--skipLibCheck', '--target', 'ES2022', '--module', 'NodeNext',
        '--moduleResolution', 'NodeNext', '--lib', 'ES2022,DOM', '--types', 'node', '--typeRoots',
        tools / 'node_modules/@types', '--outDir', '.test-output', 'mixed-client.mts')
    run('node', '.test-output/mixed-client.mjs', native_url, ts_url, timeout=30)
    run(Path(metadata['target_directory']) / 'debug/mixed-sdk-probe', native_url, ts_url, timeout=30)
    print('PASS mixed HTTP SDK export, check, lifecycle preservation, relocation and native HTTP/MCP startup', flush=True)
finally:
    for child in children:
        if child.poll() is None:
            child.terminate()
    for child in children:
        try:
            code = child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)
            raise AssertionError('Service did not shut down gracefully')
        assert code == 0, f'Service exit status {code}'
    if env.get('FORKLAUNCH_KEEP_FIXTURE'):
        print(f'Mixed SDK fixture evidence: {root}', flush=True)
    else:
        shutil.rmtree(root)
