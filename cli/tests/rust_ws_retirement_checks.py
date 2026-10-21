"""Exercise last-socket retirement, watch recovery and explicit service deletion."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

cli = sys.argv[1]
name = sys.argv[3] if '--delete' in sys.argv and len(sys.argv) > 3 else 'realtime'


def sdk_folder(service):
    import tomllib
    for project in tomllib.loads(Path('.forklaunch/manifest.toml').read_text())['projects']:
        if project['name'] == service:
            return Path(project['metadata']['cratePath']) / 'sdk/typescript'
    raise AssertionError(f'{service} missing from manifest')


# Socket artifacts live in the service's SDK folder; package ownership records
# sit beside the generated packages. Nothing is written under .forklaunch/.
sdk = sdk_folder(name)
artifact = sdk / 'socket-contract.json'
rust = Path(f'src/client-sdk/rust/{name}-websocket-sdk')
typescript = Path(f'src/client-sdk/typescript-websocket/{name}')
ownership = {
    'typescript': typescript.with_name(f'.{name}.forklaunch.json'),
    'rust': rust.with_name(f'.{name}-websocket-sdk.forklaunch.json'),
    'asyncapi': sdk / 'socket-sdk-asyncapi.json',
}
generate = [cli, 'sdk', 'generate', '--path', '.', '--from', 'socket-contract', '--target', 'all', '--service', 'realtime']


def run(command, failure=None):
    result = subprocess.run(command, capture_output=True, text=True, timeout=300)
    output = result.stdout + result.stderr
    if failure is None:
        assert result.returncode == 0, output
    else:
        assert result.returncode != 0 and failure in output, output


def workspace_has_sdk():
    import tomllib
    return f'src/client-sdk/rust/{name}-websocket-sdk' in tomllib.loads(Path('Cargo.toml').read_text())['workspace']['members']


if '--delete' in sys.argv:
    edited = rust / 'socket_chat.rs'
    edited.write_bytes(edited.read_bytes() + b'\n// user-owned retained source\n')
    manual = typescript / 'user.ts'
    manual.write_text('// user-owned file\n')
    run([cli, 'delete', 'service', name, '--path', '.', '--continue'])
    assert not workspace_has_sdk()
    assert edited.read_bytes().endswith(b'// user-owned retained source\n')
    assert manual.read_text() == '// user-owned file\n'
    assert not (typescript / 'package.json').exists()
    assert not (typescript / 'socket_chat.ts').exists()
    assert not (sdk / 'asyncapi.json').exists()
    assert not artifact.parent.exists()
    assert not any(record.exists() for record in ownership.values())
    assert sorted(str(p) for p in Path('.forklaunch').rglob('*')) == ['.forklaunch/manifest.toml']
    if name == 'stream-events':
        assert not Path('custom/renamed-sockets.json').exists()
    print('PASS explicit service deletion prunes owned socket SDK files/workspace entries and preserves user edits/files')
    sys.exit(0)

fixture = Path('src/realtime/api/controllers/socket_contract_fixture.json')
original_fixture = fixture.read_bytes()
original_artifact = artifact.read_bytes()
config = typescript / 'tsconfig.json'
original_config = config.read_bytes()
edited = typescript / 'socket_chat.ts'
manual = rust / 'user.rs'

try:
    # A temporary missing artifact cannot retire a still-registered socket.
    artifact.unlink()
    run(generate, failure='Missing socket contract artifact')
    assert (rust / 'Cargo.toml').exists() and workspace_has_sdk()
    artifact.write_bytes(original_artifact)
    manual.write_text('// user-owned file\n')
    settings = json.loads(original_config)
    settings['compilerOptions']['noUnusedLocals'] = False
    config.write_text(json.dumps(settings))
    run(generate + ['--check'])
    # Preserve ordering evidence when build-time HTTP artifacts and runtime
    # socket exports arrive in separate writes during one compilation.
    with Path('socket-retirement-watch.log').open('w+b') as log:
        watch = subprocess.Popen([cli, 'sdk', 'watch', '--path', '.', '--from', 'socket-contract', '--target', 'all', '--service', 'realtime', '--interval-ms', '100'], stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 30
            while b'Watching socket-contract artifacts' not in os.pread(log.fileno(), 1024 * 1024, 0):
                assert time.monotonic() < deadline and watch.poll() is None, os.pread(log.fileno(), 1024 * 1024, 0)
                time.sleep(0.1)
            # Watch initialization regenerates active source; edit it again
            # immediately before retiring the entire registry.
            edited.write_bytes(edited.read_bytes() + b'\n// user-owned obsolete module\n')
            empty = json.loads(original_fixture)
            empty['sockets'] = {}
            fixture.write_text(json.dumps(empty))
            run([cli, 'openapi', 'export', '--path', '.'])
            assert not artifact.exists()
            deadline = time.monotonic() + 30
            while workspace_has_sdk() or (rust / 'Cargo.toml').exists():
                assert time.monotonic() < deadline and watch.poll() is None, os.pread(log.fileno(), 1024 * 1024, 0)
                time.sleep(0.1)
            assert not (typescript / 'package.json').exists()
            assert not (sdk / 'asyncapi.json').exists()
            assert edited.read_bytes().endswith(b'// user-owned obsolete module\n')
            assert manual.read_text() == '// user-owned file\n'
            assert json.loads(config.read_text()) == settings
            for target in ['typescript', 'rust', 'asyncapi']:
                record = json.loads(ownership[target].read_text())
                assert record['files'] == {} and record['registryExported']
            run(generate + ['--check'])
            fixture.write_bytes(original_fixture)
            run([cli, 'openapi', 'export', '--path', '.'])
            deadline = time.monotonic() + 30
            while not workspace_has_sdk() or not (rust / 'Cargo.toml').exists():
                assert time.monotonic() < deadline and watch.poll() is None, os.pread(log.fileno(), 1024 * 1024, 0)
                time.sleep(0.1)
            run(generate + ['--check'])
        finally:
            watch.terminate()
            try:
                watch.wait(timeout=5)
            except subprocess.TimeoutExpired:
                watch.kill()
                watch.wait()
finally:
    fixture.write_bytes(original_fixture)
    config.write_bytes(original_config)
    if manual.exists():
        manual.unlink()
    run([cli, 'openapi', 'export', '--path', '.'])
    run(generate)

print('PASS last registered socket retirement and live watch recovery, owned SDK/AsyncAPI cleanup, Cargo membership, transient missing artifacts and preserved user files/compiler options')
