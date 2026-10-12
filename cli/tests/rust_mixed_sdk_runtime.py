#!/usr/bin/env python3
"""Shared generated clients against native HTTP/MCP and a real tsx HTTP service.

The TypeScript export hook is fixture-owned; this does not assert that mixed
application TypeScript module scaffolding or dev orchestration is complete.
"""
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import tempfile
import time

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-mixed-sdk-runtime'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
app = root / 'app'
env = {**os.environ, 'CARGO_INCREMENTAL': '0'}
children = []


def run(*args, cwd=None, timeout=1800):
    subprocess.run([str(arg) for arg in args], cwd=app if cwd is None else cwd, env=env, check=True, timeout=timeout)


def cli(*args):
    run(binary, *args, '--path', app, cwd=root)


def start(*args):
    child = subprocess.Popen([str(arg) for arg in args], cwd=app,
        env={**{key: value for key, value in env.items() if not key.startswith(('MCP_', 'OTEL_'))},
             'SERVICE_HOST': '127.0.0.1', 'HOST': '127.0.0.1', 'SERVICE_PORT': '0'},
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
    cli('init', 'application', 'mixed-sdk', '--language', 'rust', '-D', 'Mixed SDK', '-A', 'ForkLaunch', '-L', 'MIT')
    cli('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
    tools = root / 'tools'
    tools.mkdir()
    (tools / 'package.json').write_text('{"private":true}')
    run('npm', 'install', '--ignore-scripts', '--no-audit', '--no-fund', 'tsx@4.20.5',
        'typescript@5.9.3', '@types/node@22', cwd=tools)
    ts = app / 'services/typescript-api'
    ts.mkdir(parents=True)
    (ts / 'node_modules').symlink_to(tools / 'node_modules', target_is_directory=True)
    (ts / 'package.json').write_text('{"name":"typescript-api","private":true,"type":"module"}')
    (ts / 'tsconfig.json').write_text('{"compilerOptions":{"target":"ES2022","module":"NodeNext","moduleResolution":"NodeNext","strict":true}}')
    profile = {'type': 'object', 'properties': {'id': {'type': 'string'}, 'profileName': {'type': 'string'}}, 'required': ['id', 'profileName']}
    def response(schema):
        return {'description': 'Fixture response', 'content': {'application/json': {'schema': schema}}}
    spec = {'openapi': '3.1.0', 'info': {'title': 'TypeScript profiles', 'version': '1'}, 'paths': {
        '/profiles/{id}': {'get': {'operationId': 'getProfile', 'parameters': [
            {'name': 'id', 'in': 'path', 'required': True, 'schema': {'type': 'string'}}],
            'responses': {'200': response({'$ref': '#/components/schemas/Profile'}),
                          '404': response({'type': 'object', 'properties': {'errorCode': {'type': 'string'}}, 'required': ['errorCode']})}}},
        '/profiles': {'post': {'operationId': 'createProfile', 'requestBody': {'required': True, 'content': {
            'application/json': {'schema': {'type': 'object', 'properties': {'profileName': {'type': 'string'}}, 'required': ['profileName']}}}},
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
    manifest = app / '.forklaunch/manifest.toml'
    text = manifest.read_text().replace('modules_path = "crates"', 'modules_path = "services"')
    text = re.sub(r'^language = "rust"$', 'language = "mixed"', text, count=1, flags=re.M)
    text = re.sub(r'^applicationLanguage = "rust"$', 'applicationLanguage = "mixed"', text, count=1, flags=re.M)
    manifest.write_text(text + '\n[[projects]]\nname = "typescript-api"\ntype = "Service"\ndescription = "Owned tsx fixture"\n[projects.metadata]\nlanguage = "typescript"\n')
    cli('openapi', 'export')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    # Lifecycle must retain a previously exported TypeScript client.
    cli('sync', 'service', 'native-api')
    assert 'typescriptApi:' in (app / 'sdks/typescript/src/index.ts').read_text()
    cli('sdk', 'generate', '--target', 'all', '--from', 'openapi')
    # Source tracking and generated packages must survive relocation.
    relocated = root / 'relocated-app'
    app.rename(relocated)
    app = relocated
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    # Watch both the manifest and a relocated TS export even in contract mode.
    sdk_source = app / 'sdks/typescript/src/index.ts'
    manifest = app / '.forklaunch/manifest.toml'
    artifact = app / '.forklaunch/openapi/typescript-api/openapi.json'
    custom = app / 'artifacts/typescript/spec.json'
    custom.parent.mkdir(parents=True)
    artifact.rename(custom)
    manifest.write_text(manifest.read_text() + 'openapiPath = "artifacts/typescript/spec.json"\n')
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract')
    watch_log = root / 'watch.log'
    with watch_log.open('w') as log:
        watcher = subprocess.Popen([str(binary), 'sdk', 'watch', '--path', str(app),
            '--from', 'contract', '--interval-ms', '100'], cwd=app, env=env, stdout=log, stderr=log)
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
            custom.write_text(json.dumps(changed))
            wait_for(lambda: 'watchMarker' in sdk_source.read_text())
            original = sdk_source.read_bytes()
            custom.unlink()
            wait_for(lambda: 'SDK regeneration failed' in watch_log.read_text())
            assert sdk_source.read_bytes() == original
            # Deleting a disposable native service refreshes the existing SDKs.
            cli('init', 'service', 'disposable-api', '--language', 'rust', '-D', 'Lifecycle fixture')
            failed = subprocess.run([str(binary), 'delete', 'service', 'disposable-api', '--continue', '--path', str(app)],
                cwd=app, env=env, capture_output=True, text=True, timeout=60)
            assert failed.returncode != 0 and 'Restore the TypeScript OpenAPI export' in failed.stderr + failed.stdout
            assert sdk_source.read_bytes() == original
            changed['components']['schemas']['Profile']['properties']['recoveredMarker'] = {'type': 'boolean'}
            custom.write_text(json.dumps(changed))
            wait_for(lambda: 'recoveredMarker' in sdk_source.read_text())
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
    print('PASS mixed SDK watch follows custom TS exports, preserves outputs on loss and recovers', flush=True)
    probe = app / 'mixed-probe'
    (probe / 'src').mkdir(parents=True)
    (probe / 'Cargo.toml').write_text('''[package]
name = "mixed-sdk-probe"
version = "0.0.0"
edition = "2024"
[dependencies]
native-api-sdk = { path = "../sdks/rust/native-api-sdk" }
typescript-api-sdk = { path = "../sdks/rust/typescript-api-sdk" }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
''')
    (probe / 'src/main.rs').write_text((repo / 'cli/tests/fixtures/rust_mixed_sdk.rs').read_text())
    workspace = app / 'Cargo.toml'
    workspace.write_text(workspace.read_text().replace('members = [', 'members = ["mixed-probe", '))
    run('cargo', 'build', '-p', 'native-api', '-p', 'mixed-sdk-probe')
    run('cargo', 'clippy', '-p', 'native-api-sdk', '-p', 'typescript-api-sdk', '-p', 'mixed-sdk-probe', '--', '-D', 'warnings')
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=app, env=env))
    native = Path(metadata['target_directory']) / 'debug/native-api'
    run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', native)
    native_url = start(native)
    ts_url = start(tools / 'node_modules/.bin/tsx', app / 'services/typescript-api/server.ts')
    (app / 'mixed-client.mts').write_text('''import assert from 'node:assert/strict';
import {createForkLaunchSdk} from './sdks/typescript/src/index.js';
const seen:string[]=[];
const sdk=createForkLaunchSdk({serviceBaseUrls:{'native-api':process.argv[2]!,'typescript-api':process.argv[3]!},
 beforeRequest(request){seen.push(request.service);return {...request,input:{...request.input,headers:{'x-sdk-consumer':'typescript'}}};}});
const health=await sdk.nativeApi.health.getHealth();
assert.equal(health.code,200);if(health.code===200)assert.equal(health.response.status,'ok');
const id='Ada / 🦀 & backend';
const profile=await sdk.typescriptApi.profiles.getProfile({params:{id}});
assert.equal(profile.code,200);if(profile.code===200){assert.equal(profile.response.id,id);assert.equal(profile.response.profileName,'Ada');}
const missing=await sdk.typescriptApi.profiles.getProfile({params:{id:'missing'}});
assert.equal(missing.code,404);if(missing.code===404)assert.equal(missing.response.errorCode,'not_found');
const created=await sdk.typescriptApi.profiles.createProfile({body:{profileName:'Grace'}});
assert.equal(created.code,201);if(created.code===201)assert.equal(created.response.profileName,'Grace');
assert.deepEqual(seen,['native-api','typescript-api','typescript-api','typescript-api']);
if(false){
 // @ts-expect-error path parameters are required
 sdk.typescriptApi.profiles.getProfile();
 // @ts-expect-error the request body comes from the TypeScript OpenAPI schema
 sdk.typescriptApi.profiles.createProfile({body:{profileName:42}});
 if(profile.code===200){
  // @ts-expect-error undeclared response field
  profile.response.undeclared;
 }
}
console.log('PASS one strict TypeScript SDK, live Rust/TypeScript requests, escaped params, typed errors, bodies and hooks');
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
    print(f'Mixed SDK fixture evidence: {root}', flush=True)
