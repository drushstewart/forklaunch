"""Verify CLI socket generation stages all services and honors configured artifacts."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

cli = sys.argv[1]
artifact = Path('.forklaunch/socket-contracts/realtime/socket-contract.json')
original = artifact.read_bytes()
manifest = Path('.forklaunch/manifest.toml')
original_manifest = manifest.read_text()
command = [cli, 'sdk', 'generate', '--path', '.', '--target', 'all', '--from', 'socket-contract']


def run(args=(), failure=None):
    result = subprocess.run(command + list(args), capture_output=True, text=True, timeout=300)
    if failure is None:
        assert result.returncode == 0, result.stdout + result.stderr
    else:
        assert result.returncode != 0, result.stdout + result.stderr
        assert failure in result.stdout + result.stderr, result.stdout + result.stderr
    return result


def snapshot():
    paths = [Path('Cargo.toml'), Path('src/pnpm-workspace.yaml')]
    for directory in ['src/client-sdk', '.forklaunch/asyncapi', '.forklaunch/socket-sdk-outputs']:
        paths += [p for p in Path(directory).rglob('*') if p.is_file()]
    return {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


baseline = snapshot()
try:
    source = Path('src/client-sdk/typescript-websocket/realtime/socket_chat.ts')
    content = source.read_bytes()
    source.write_bytes(content + b'\n// drift\n')
    run(['--check'], failure='out of date')
    assert source.read_bytes() == content + b'\n// drift\n'
    run()
    assert snapshot() == baseline
    # Removed sockets must not survive as stale generated source modules.
    reduced = json.loads(original)
    removed = [name for name in reduced['sockets'] if name != 'chat']
    assert len(removed) >= 2
    for name in removed:
        del reduced['sockets'][name]
    preserved = Path(f'src/client-sdk/typescript-websocket/realtime/socket_{removed[0]}.ts')
    preserved.write_bytes(preserved.read_bytes() + b'\n// user-owned obsolete module\n')
    manual = Path('src/client-sdk/rust/realtime-websocket-sdk/user.rs')
    manual.write_text('// user-owned source\n')
    artifact.write_text(json.dumps(reduced))
    before = snapshot()
    run(['--check'], failure='Stale generated socket SDK output')
    assert snapshot() == before
    run()
    run(['--check'])
    for name in removed:
        assert not Path(f'src/client-sdk/rust/realtime-websocket-sdk/socket_{name}.rs').exists()
        source = Path(f'src/client-sdk/typescript-websocket/realtime/socket_{name}.ts')
        if source == preserved:
            assert source.read_bytes().endswith(b'// user-owned obsolete module\n')
        else:
            assert not source.exists()
    assert manual.read_text() == '// user-owned source\n'
    manual.unlink()
    artifact.write_bytes(original)
    run()
    assert snapshot() == baseline
    invalid = json.loads(original)
    invalid['sockets']['chat']['clientMessages']['request']['shape']['properties']['message']['maxLength'] = 'invalid'
    artifact.write_text(json.dumps(invalid))
    run(failure='Invalid socket')
    assert snapshot() == baseline
    artifact.write_bytes(original)

    # A later invalid service must prevent earlier staged changes from being written.
    valid = json.loads(original)
    valid['title'] = 'Staged title that must never be written'
    del valid['sockets']['chat']
    artifact.write_text(json.dumps(valid))
    broken = Path('.forklaunch/socket-contracts/broken/socket-contract.json')
    broken.parent.mkdir(parents=True)
    broken.write_text(json.dumps(invalid))
    manifest.write_text(original_manifest + '\n[[projects]]\nname = "broken"\ntype = "Service"\ndescription = "Broken fixture"\nrouters = []\n')
    run(failure='Invalid socket')
    assert snapshot() == baseline
    manifest.write_text(original_manifest)
    artifact.write_bytes(original)

    with tempfile.TemporaryDirectory(prefix='fl-socket-outside-') as outside:
        external = Path(outside) / 'contract.json'
        external.write_bytes(original)
        manifest.write_text(original_manifest.replace('[projects.metadata]\n', '[projects.metadata]\nsocketContractArtifactPath = ' + json.dumps(str(external)) + '\n'))
        run(failure='must stay inside the application')
        assert snapshot() == baseline
        manifest.write_text(original_manifest)
        directory = Path('src/client-sdk/typescript-websocket/realtime')
        saved = directory.with_name('realtime.saved')
        directory.rename(saved)
        directory.symlink_to(outside, target_is_directory=True)
        try:
            before = snapshot()
            run(failure='must stay inside the application')
            assert snapshot() == before
            assert sorted(p.name for p in Path(outside).iterdir()) == ['contract.json']
        finally:
            directory.unlink()
            saved.rename(directory)

    custom = Path('custom/socket-contract.json')
    custom.parent.mkdir(parents=True, exist_ok=True)
    custom.write_bytes(original)
    custom_manifest = original_manifest.replace('[projects.metadata]\n', '[projects.metadata]\nsocketContractArtifactPath = "custom/socket-contract.json"\n')
    custom_manifest = custom_manifest.replace('asyncapiPath = ".forklaunch/asyncapi/realtime/asyncapi.json"', 'asyncapiPath = "custom/asyncapi.json"')
    manifest.write_text(custom_manifest)
    run()
    run(['--check'])
    output = Path('custom/asyncapi.json')
    assert json.loads(output.read_text())['info']['title'] == json.loads(original)['title']
    with tempfile.TemporaryFile() as log:
        watch = subprocess.Popen([cli, 'sdk', 'watch', '--path', '.', '--service', 'realtime', '--target', 'all', '--from', 'socket-contract', '--interval-ms', '100'], stdout=log, stderr=log)
        try:
            ready = time.monotonic() + 30
            while time.monotonic() < ready:
                if b'Watching socket-contract artifacts' in os.pread(log.fileno(), 1024 * 1024, 0):
                    break
                assert watch.poll() is None, 'Socket watch stopped before initialization'
                time.sleep(0.1)
            else:
                raise AssertionError('Socket watch did not initialize: ' + os.pread(log.fileno(), 1024 * 1024, 0).decode(errors='replace'))
            manifest.write_text(custom_manifest + '\n[temporarily invalid')
            rejected_manifest = time.monotonic() + 30
            while time.monotonic() < rejected_manifest:
                if b'Unable to read SDK artifacts' in os.pread(log.fileno(), 1024 * 1024, 0):
                    break
                assert watch.poll() is None, 'Socket watch stopped on temporarily invalid manifest metadata'
                time.sleep(0.1)
            else:
                raise AssertionError('Invalid watched manifest was not diagnosed')
            time.sleep(0.3)
            assert watch.poll() is None, 'Socket watch stopped on temporarily invalid manifest metadata'
            assert os.pread(log.fileno(), 1024 * 1024, 0).count(b'Unable to read SDK artifacts') == 1
            manifest.write_text(custom_manifest)
            custom.write_text(json.dumps(invalid))
            rejected = time.monotonic() + 30
            while time.monotonic() < rejected:
                if b'Invalid socket' in os.pread(log.fileno(), 1024 * 1024, 0):
                    break
                assert watch.poll() is None, 'Socket watch stopped on a temporarily invalid contract'
                time.sleep(0.1)
            else:
                raise AssertionError('Invalid watched contract was not diagnosed')
            time.sleep(0.3)
            assert watch.poll() is None, 'Socket watch stopped on a temporarily invalid contract'
            assert json.loads(output.read_text())['info']['title'] == json.loads(original)['title']
            changed = json.loads(original)
            changed['title'] = 'Watched custom artifact'
            custom.write_text(json.dumps(changed))
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                try:
                    if json.loads(output.read_text())['info']['title'] == changed['title']:
                        break
                except (json.JSONDecodeError, OSError):
                    pass
                assert watch.poll() is None, 'Socket watch stopped before regeneration'
                time.sleep(0.1)
            else:
                raise AssertionError('Custom socket artifact was not regenerated')
        finally:
            watch.terminate()
            try:
                watch.wait(timeout=5)
            except subprocess.TimeoutExpired:
                watch.kill()
                watch.wait()

finally:
    manifest.write_text(original_manifest)
    artifact.write_bytes(original)
    run()
print('PASS CLI socket drift checks, owned obsolete module pruning with preserved edits, all-service staging, input/output containment, custom artifact/AsyncAPI paths and live watch recovery after invalid edits')
