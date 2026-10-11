"""Exercise last-socket retirement, watch recovery and explicit service deletion."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

cli = sys.argv[1]
name = sys.argv[3] if '--delete' in sys.argv and len(sys.argv) > 3 else 'realtime'
artifact = Path(f'.forklaunch/socket-contracts/{name}/socket-contract.json')
rust = Path(f'sdks/rust/{name}-websocket-sdk')
typescript = Path(f'sdks/typescript-websocket/{name}')
ownership = Path(f'.forklaunch/socket-sdk-outputs/{name}')
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
    return f'sdks/rust/{name}-websocket-sdk' in tomllib.loads(Path('Cargo.toml').read_text())['workspace']['members']


if '--delete' in sys.argv:
    edited = rust / 'src/socket_chat.rs'
    edited.write_bytes(edited.read_bytes() + b'\n// user-owned retained source\n')
    manual = typescript / 'src/user.ts'
    manual.write_text('// user-owned file\n')
    run([cli, 'delete', 'service', name, '--path', '.', '--continue'])
    assert not workspace_has_sdk()
    assert edited.read_bytes().endswith(b'// user-owned retained source\n')
    assert manual.read_text() == '// user-owned file\n'
    assert not (typescript / 'package.json').exists()
    assert not (typescript / 'src/socket_chat.ts').exists()
    assert not Path(f'.forklaunch/asyncapi/{name}').exists()
    assert not artifact.parent.exists()
    assert not list(ownership.glob('*.json'))
    if name == 'stream-events':
        assert not Path('custom/renamed-sockets.json').exists()
    print('PASS explicit service deletion prunes owned socket SDK files/workspace entries and preserves user edits/files')
    sys.exit(0)

fixture = Path('crates/realtime/src/socket_contract_fixture.json')
original_fixture = fixture.read_bytes()
original_artifact = artifact.read_bytes()
config = typescript / 'tsconfig.json'
original_config = config.read_bytes()
edited = typescript / 'src/socket_chat.ts'
manual = rust / 'src/user.rs'

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
    with tempfile.TemporaryFile() as log:
        watch = subprocess.Popen([cli, 'sdk', 'watch', '--path', '.', '--from', 'socket-contract', '--target', 'all', '--service', 'realtime', '--interval-ms', '100'], stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 30
            while b'Watching Rust' not in os.pread(log.fileno(), 1024 * 1024, 0):
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
            assert not Path('.forklaunch/asyncapi/realtime/asyncapi.json').exists()
            assert edited.read_bytes().endswith(b'// user-owned obsolete module\n')
            assert manual.read_text() == '// user-owned file\n'
            assert json.loads(config.read_text()) == settings
            for target in ['typescript', 'rust', 'asyncapi']:
                record = json.loads((ownership / f'{target}.json').read_text())
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
