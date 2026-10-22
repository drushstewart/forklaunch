#!/usr/bin/env python3
"""Current socket registry lifecycle with actual TypeScript and Rust peers."""
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib

repo = Path(sys.argv[1]).resolve()
fixtures = repo / 'cli/tests/fixtures'
root = Path(tempfile.mkdtemp(prefix='forklaunch-ws-cli-')).resolve()
cli = root / 'forklaunch'
shutil.copy2(os.environ['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA-256:', digest, flush=True)
env = dict(os.environ)
app = root / 'original'
passed = False


def run(*args, cwd=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd or app, env=env, check=True, timeout=1800)


def command(*args):
    run(cli, *args, '--path', app, cwd=root)


try:
    command('init', 'application', 'realtime-app', '--database', 'postgresql', '--validator', 'zod',
            '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express', '--runtime', 'node',
            '--test-framework', 'vitest', '-D', 'Realtime', '-A', 'ForkLaunch', '-L', 'MIT')
    command('init', 'service', 'realtime', '--language', 'rust', '-D', 'Realtime')
    relocated = root / 'relocated-app'
    app.rename(relocated)
    app = relocated
    service = app / 'src/realtime'
    declaration = service / 'api/controllers/socket_contracts.rs'
    shutil.copy2(fixtures / 'rust_socket_contract.json', declaration.with_name('socket_contract_fixture.json'))
    declaration.write_text('''pub fn artifact() -> forklaunch_ws::SocketContractArtifact {
    serde_json::from_str(include_str!("socket_contract_fixture.json")).unwrap()
}
''')
    host = service / '.forklaunch/contracts/lib.rs'
    host.write_text('#[path="../../api/controllers/socket_contracts.rs"]\npub mod socket_fixture;\n' +
                    host.read_text().replace('//!', '//').replace('pub fn registry()', 'pub fn native_registry()')
                    .replace('pub fn binding_manifest()', 'pub fn native_binding_manifest()') + '''
pub fn registry() -> forklaunch_core::contract::ContractRegistry {
    let mut registry = native_registry();
    for (name, socket) in socket_fixture::artifact().sockets {
        registry.register_socket(name, &socket).unwrap();
    }
    registry
}
pub fn binding_manifest() -> Result<Vec<forklaunch_core::contract::assembly::BindingDeclaration>, String> {
    let mut declarations = native_binding_manifest()?;
    for (name, socket) in socket_fixture::artifact().sockets {
        declarations.push(forklaunch_core::contract::assembly::BindingDeclaration::socket(
            "api/controllers/socket_contracts.rs", "socket_fixture", &name, &socket,
        )?);
    }
    Ok(declarations)
}
''')
    # These declarations describe the external TypeScript peer exercised below.
    # Keep the generated Rust host's runtime registry limited to its bound HTTP
    # health endpoint; the private compiler registry still emits every socket
    # fixture for the CLI's export/watch/retirement and both client compilers.
    routes = service / 'api/routes/mod.rs'
    routes.write_text(routes.read_text().replace(
        'ContractRouter::new(contracts::registry())',
        'ContractRouter::new(contracts::native_registry())'))
    application = service / 'application.rs'
    application.write_text(application.read_text().replace(
        'contracts::registry()', 'contracts::native_registry()'))
    host_cargo = host.parent / 'Cargo.toml'
    # The framework comes from the registry release the manifest selects.
    framework_version = tomllib.loads((app / '.forklaunch/manifest.toml').read_text())['framework']['version']
    host_cargo.write_text(host_cargo.read_text().replace('[dependencies]\n', '[dependencies]\nserde_json = "1"\n') +
                         f'\n[dependencies.forklaunch-ws]\nversion = "={framework_version}"\n')
    run('cargo', 'build', '--workspace')
    command('openapi', 'export')
    expected = json.loads(declaration.with_name('socket_contract_fixture.json').read_text())
    artifact = json.loads((service / 'sdk/typescript/socket-contract.json').read_text())
    contract = json.loads((service / 'sdk/typescript/contract.json').read_text())
    for socket in expected['sockets'].values():
        socket.setdefault('clientMessages', {})
        socket.setdefault('serverMessages', {})
    assert artifact['sockets'] == expected['sockets'] == contract['sockets']
    assert artifact['title'] == contract['service']['name']
    assert artifact['version'] == contract['service']['version']
    run('python3', repo / 'cli/tests/rust_ws_registry_checks.py', cli)
    command('sdk', 'generate', '--target', 'all', '--from', 'socket-contract')
    command('sdk', 'generate', '--target', 'all', '--from', 'socket-contract', '--check')
    command('sdk', 'watch', '--target', 'all', '--from', 'socket-contract', '--service', 'realtime', '--once')
    run('python3', repo / 'cli/tests/rust_ws_cli_checks.py', cli)
    run('python3', repo / 'cli/tests/rust_ws_retirement_checks.py', cli)

    sdk = app / 'src/client-sdk/typescript-websocket/realtime'
    rust = app / 'src/client-sdk/rust/realtime-websocket-sdk'
    assert (sdk / 'socket_chat.ts').is_file() and not (sdk / 'src').exists()
    assert (rust / 'lib.rs').is_file() and not (rust / 'src').exists()
    run('npm', 'install', '--prefix', sdk, '--ignore-scripts', '--package-lock=false')
    env['TSC_BIN'] = os.environ.get('TSC_BIN', str(sdk / 'node_modules/.bin/tsc'))
    if not env.get('FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES'):
        tools = root / 'tools'
        tools.mkdir()
        (tools / 'package.json').write_text('{"private":true}')
        run('npm', 'install', '--prefix', tools, '--ignore-scripts', '--package-lock=false',
            '@asyncapi/parser@3.6.3', 'ws@8.22.0', 'zod@4.6.5', 'ts-deepmerge@8.0.0')
        env['FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES'] = str(tools)
    config = sdk / 'tsconfig.json'
    value = json.loads(config.read_text())
    value['compilerOptions']['noUnusedLocals'] = False
    config.write_text(json.dumps(value))
    command('sdk', 'generate', '--target', 'all', '--from', 'socket-contract', '--check')
    run(env['TSC_BIN'], '--project', config)
    run('cargo', 'rustc', '-p', 'realtime-websocket-sdk', '--lib', '--', '-D', 'warnings')
    manifest = rust / 'Cargo.toml'
    manifest.write_text(manifest.read_text() + '\n[dev-dependencies]\ntokio={version="1",features=["macros","rt-multi-thread"]}\n')
    (rust / 'examples').mkdir()
    shutil.copy2(fixtures / 'rust_ws_cli_roundtrip.rs', rust / 'examples/roundtrip.rs')
    run('cargo', 'build', '-p', 'realtime-websocket-sdk', '--example', 'roundtrip')
    run('cargo', 'build', '-p', 'realtime')
    target = Path(env['CARGO_TARGET_DIR']) / 'debug'
    run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', target / 'realtime')
    run('node', repo / 'cli/tests/rust_ws_cli_runtime.mjs', app, target / 'examples/roundtrip')
    run('python3', repo / 'cli/tests/rust_ws_rename_checks.py', cli)
    run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', target / 'stream-events')
    run('python3', repo / 'cli/tests/rust_ws_retirement_checks.py', cli, '--delete', 'stream-events')
    print('PASS current relocated CLI socket registry/retirement/watch/containment/rename, actual peers and HTTP/MCP startup', flush=True)
    if os.environ.get('FORKLAUNCH_WS_CLI_RECEIPT'):
        Path(os.environ['FORKLAUNCH_WS_CLI_RECEIPT']).write_text(json.dumps({
            'status': 'PASS', 'cliSha256': digest,
            'checks': ['raw neutral init and relocation', 'private compiler-host socket declarations',
                       'flat native TS/Rust socket SDKs', 'registry ownership and last-socket retirement',
                       'watch recovery and symlink containment', 'actual TypeScript peer and official AsyncAPI parser',
                       'auth/subprotocols/Unicode/ping/pong/close', 'rename/delete ownership', 'HTTP/MCP startup'],
        }, indent=2) + '\n')
    passed = True
finally:
    if passed and not os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
