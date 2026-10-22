#!/usr/bin/env python3
"""Import AsyncAPI into a current raw app and verify real typed socket clients."""
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
fixtures = repo / 'cli/tests/fixtures'
root = Path(tempfile.mkdtemp(prefix='forklaunch-asyncapi-')).resolve()
cli = root / 'forklaunch'
shutil.copy2(os.environ['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA256:', digest, flush=True)
app = root / 'original'
env = dict(os.environ)
registry = env.get('FORKLAUNCH_TEST_REGISTRY_SOCKET') == '1'
cross = env.get('FORKLAUNCH_TEST_ASYNCAPI_CROSS_CHANNEL') == '1'
resources = env.get('FORKLAUNCH_TEST_ASYNCAPI_RESOURCES') == '1'
passed = False


def run(*args, cwd=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd or app, env=env, check=True, timeout=1800)


def command(*args):
    run(cli, *args, '--path', app, cwd=root)


try:
    command('init', 'application', 'import-app', '--database', 'postgresql', '--validator', 'zod',
            '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express', '--runtime', 'node',
            '--test-framework', 'vitest', '-D', 'AsyncAPI', '-A', 'ForkLaunch', '-L', 'MIT')
    command('init', 'service', 'realtime', '--language', 'rust', '-D', 'Realtime')
    relocated = root / 'relocated'
    app.rename(relocated)
    app = relocated
    run('cargo', 'build', '-p', 'realtime')
    if registry:
        assert not (app / 'vendor/framework').exists()
        assert not (app / 'vendor/serde_json').exists()
    artifact = app / 'src/realtime/sdk/typescript/asyncapi.json'
    artifact.parent.mkdir(parents=True, exist_ok=True)
    document = json.loads((fixtures / 'rust_asyncapi_socket.json').read_text())
    if resources:
        for name, schema in document['components']['schemas'].items():
            schema['$id'] = f'https://schemas.fixture.test/{name}'
    if cross:
        reply = document['channels']['chat']['messages'].pop('reply')
        document['channels']['responses'] = {'address':'/responses','messages':{'reply':reply}}
        document['components']['replies']['response'] = {
            'channel':{'$ref':'#/channels/responses'}, 'messages':[{'$ref':'#/channels/responses/messages/reply'}]}
    artifact.write_text(json.dumps(document))
    command('sdk', 'generate', '--target', 'all', '--from', 'asyncapi')
    command('sdk', 'generate', '--target', 'all', '--from', 'asyncapi', '--check')
    command('sdk', 'watch', '--target', 'all', '--from', 'asyncapi', '--service', 'realtime', '--once')
    if not cross:
        run('python3', repo / 'cli/tests/rust_asyncapi_import_checks.py', cli)
    sdk = app / 'src/client-sdk/typescript-websocket/realtime'
    run('npm', 'install', '--prefix', sdk, '--ignore-scripts', '--package-lock=false')
    tools = root / 'tools'
    tools.mkdir()
    (tools / 'package.json').write_text('{"private":true}')
    run('npm', 'install', '--prefix', tools, '--ignore-scripts', '--package-lock=false', '@asyncapi/parser', 'ws')
    env['FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES'] = str(tools)
    env['TSC_BIN'] = os.environ.get('TSC_BIN', str(sdk / 'node_modules/.bin/tsc'))
    run(env['TSC_BIN'], '--project', sdk / 'tsconfig.json')
    rust = app / 'src/client-sdk/rust/realtime-websocket-sdk'
    manifest = rust / 'Cargo.toml'
    if registry:
        dependency = tomllib.loads(manifest.read_text())['dependencies']['forklaunch-ws']
        assert dependency['version'] == '=0.1.0' and 'path' not in dependency
    manifest.write_text(manifest.read_text() + '\n[dev-dependencies]\ntokio={version="1",features=["macros","rt-multi-thread"]}\n')
    example = 'cross_reply' if cross else 'import'
    (rust / 'examples').mkdir()
    shutil.copy2(fixtures / ('rust_asyncapi_cross_reply.rs' if cross else 'rust_asyncapi_client.rs'), rust / 'examples' / (example + '.rs'))
    run('cargo', 'clippy', '--manifest-path', manifest, '--all-targets', '--', '-D', 'warnings')
    run('cargo', 'build', '--manifest-path', manifest, '--example', example)
    target = Path(env['CARGO_TARGET_DIR']) / 'debug'
    run('node', repo / ('cli/tests/rust_asyncapi_cross_channel_runtime.mjs' if cross else 'cli/tests/rust_asyncapi_import_runtime.mjs'), app, target / 'examples' / example)
    run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', target / 'realtime')
    if not cross:
        run('python3', repo / 'cli/tests/rust_asyncapi_import_checks.py', cli, '--rename')
        run('cargo', 'build', '-p', 'stream-events')
        run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', target / 'stream-events')
        run('python3', repo / 'cli/tests/rust_asyncapi_import_checks.py', cli, '--delete')
    if os.environ.get('FORKLAUNCH_ASYNCAPI_RECEIPT'):
        Path(os.environ['FORKLAUNCH_ASYNCAPI_RECEIPT']).write_text(json.dumps({
            'schemaVersion':1, 'cliSha256':digest, 'passed':True, 'crossChannel':cross, 'schemaResources':resources, 'publishedRegistry':registry,
            'checks':['neutral raw application', 'relocated native service', 'actual SDK builds', 'official AsyncAPI parser',
                      'live typed Rust and TypeScript peers', 'message direction and constraints', 'HTTP/MCP startup'] +
                     ([] if cross else ['staged writes and invalid-input purity', 'watch recovery', 'rename/delete ownership']),
        }, indent=2) + '\n')
    passed = True
finally:
    if passed and not os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
