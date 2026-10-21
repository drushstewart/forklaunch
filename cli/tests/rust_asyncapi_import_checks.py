"""Exercise imported document ownership, staged writes, watch and service rename."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import tomllib

cli = sys.argv[1]
manifest = Path('.forklaunch/manifest.toml')


def run(args, failure=False):
    result = subprocess.run([cli] + args, capture_output=True, text=True, timeout=300)
    assert (result.returncode != 0) == failure, result.stdout + result.stderr
    return result.stdout + result.stderr


def generate(name='realtime'):
    return ['sdk', 'generate', '--path', '.', '--from', 'asyncapi', '--target', 'all', '--service', name]


def snapshot():
    roots = [Path('src/client-sdk'), Path('.forklaunch/socket-sdk-outputs')]
    return {str(p): p.read_bytes() for root in roots for p in root.rglob('*')
            if p.is_file() and not p.is_symlink() and 'node_modules' not in p.parts}


def wait_for(predicate, output):
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(.1)
    output.seek(0)
    raise AssertionError(output.read())


if '--delete' in sys.argv:
    custom = Path('custom/server-asyncapi.json')
    original = custom.read_bytes()
    run(['delete', 'service', 'stream-events', '--path', '.', '--continue'])
    assert custom.read_bytes() == original
    assert not Path('.forklaunch/socket-sdk-outputs/stream-events').exists()
    assert not Path('src/client-sdk/rust/stream-events-websocket-sdk/socket_chat.rs').exists()
    assert not Path('src/client-sdk/typescript-websocket/stream-events/socket_chat.ts').exists()
    assert Path('src/client-sdk/rust/realtime-websocket-sdk/socket_chat.rs').read_bytes().endswith(b'// preserved user edits\n')
    assert 'src/client-sdk/rust/stream-events-websocket-sdk' not in tomllib.loads(Path('Cargo.toml').read_text())['workspace']['members']
    print('PASS imported service deletion removes owned SDKs and preserves custom inputs and edited old files')
elif '--rename' not in sys.argv:
    artifact = Path('.forklaunch/asyncapi/realtime/asyncapi.json')
    original = artifact.read_bytes()
    before = snapshot()
    assert not Path('.forklaunch/socket-sdk-outputs/realtime/asyncapi.json').exists()
    for target in ['rust', 'typescript']:
        marker = Path(f'.forklaunch/socket-sdk-outputs/realtime/{target}.json')
        assert json.loads(marker.read_text())['source'] == 'asyncapi'
    # Schema pointers must use literal separators in the checked profile.
    for reference in ['#%2Fcomponents/schemas/Request', '#/components%2Fschemas/Request']:
        invalid = json.loads(original)
        invalid['components']['messages']['Request']['payload']['$ref'] = reference
        artifact.write_text(json.dumps(invalid))
        edited_input = artifact.read_bytes()
        try:
            for flags in [[], ['--check']]:
                output = run(generate() + flags, failure=True)
                assert 'literal JSON-pointer separators' in output, output
                assert snapshot() == before
                assert artifact.read_bytes() == edited_input
        finally:
            artifact.write_bytes(original)
    # Unsupported reply routing must fail before mutating either generated SDK.
    for mutation in ['dynamic', 'foreign-message', 'cycle', 'missing-channel']:
        invalid = json.loads(original)
        reply = invalid['components']['replies']['response']
        if mutation == 'dynamic':
            reply['address'] = {'location': '$message.header#/replyTo'}
        elif mutation == 'foreign-message':
            reply['messages'] = [{'$ref': '#/components/messages/Reply'}]
        elif mutation == 'cycle':
            invalid['components']['replies']['response'] = {'$ref': '#/components/replies/response'}
        else:
            del reply['channel']
        artifact.write_text(json.dumps(invalid))
        edited_input = artifact.read_bytes()
        try:
            for flags in [[], ['--check']]:
                run(generate() + flags, failure=True)
                assert snapshot() == before
                assert artifact.read_bytes() == edited_input
        finally:
            artifact.write_bytes(original)
    # Regenerating one language from native metadata must protect the other
    # language's imported document, including the shared default input path.
    socket = Path('.forklaunch/socket-contracts/realtime/socket-contract.json')
    socket.parent.mkdir(parents=True, exist_ok=True)
    socket.write_text(json.dumps({'formatVersion': 1, 'title': 'Native', 'version': '1.0.0', 'sockets': {
        'chat': {'path': '/chat', 'clientMessages': {'request': {'shape': {'type': 'object'}}},
                 'serverMessages': {'reply': {'shape': {'type': 'object'}}}}
    }}))
    try:
        output = run(['sdk', 'generate', '--path', '.', '--from', 'socket-contract', '--target', 'rust', '--service', 'realtime'], failure=True)
        assert 'overwrite an input artifact' in output, output
        assert artifact.read_bytes() == original
        assert snapshot() == before
    finally:
        socket.unlink()
    # An invalid later service must prevent writes for earlier valid services.
    original_manifest = manifest.read_bytes()
    changed = json.loads(original)
    changed['components']['schemas']['Other']['properties']['count']['minimum'] = 2
    artifact.write_text(json.dumps(changed))
    other = Path('.forklaunch/asyncapi/invalid/asyncapi.json')
    other.parent.mkdir(parents=True)
    other.write_text('{}')
    manifest.write_bytes(original_manifest + b'\n[[projects]]\nname = "invalid"\ntype = "Service"\n')
    try:
        run(['sdk', 'generate', '--path', '.', '--from', 'asyncapi', '--target', 'all'], failure=True)
        assert snapshot() == before
    finally:
        manifest.write_bytes(original_manifest)
        artifact.write_bytes(original)
        other.unlink()
        other.parent.rmdir()
    # A valid input placed at a generated destination must never be overwritten.
    package = Path('src/client-sdk/typescript-websocket/realtime/package.json')
    original_package = package.read_bytes()
    package.write_bytes(original)
    manifest.write_text(manifest.read_text().replace('[projects.metadata]\n', '[projects.metadata]\nasyncapiArtifactPath = "src/client-sdk/typescript-websocket/realtime/package.json"\n'))
    try:
        output = run(generate(), failure=True)
        assert 'overwrite an input artifact' in output, output
        assert package.read_bytes() == original
    finally:
        package.write_bytes(original_package)
        manifest.write_bytes(original_manifest)
    # Watch an explicitly configured input, recovering after invalid edits.
    custom = Path('custom/server-asyncapi.json')
    custom.parent.mkdir()
    custom.write_bytes(original)
    manifest.write_text(manifest.read_text().replace('[projects.metadata]\n', '[projects.metadata]\nasyncapiArtifactPath = "custom/server-asyncapi.json"\n'))
    run(generate())
    marker = Path('.forklaunch/socket-sdk-outputs/realtime/rust.json')
    initial = marker.read_bytes()
    with tempfile.TemporaryFile(mode='w+') as output:
        watch = subprocess.Popen([cli, 'sdk', 'watch', '--path', '.', '--from', 'asyncapi', '--target', 'all', '--service', 'realtime', '--interval-ms', '100'], stdout=output, stderr=output)
        try:
            def started():
                output.seek(0)
                return 'Watching' in output.read()
            wait_for(started, output)
            custom.write_text('{}')
            def failed():
                output.seek(0)
                return 'failed' in output.read().lower()
            wait_for(failed, output)
            assert marker.read_bytes() == initial
            custom.write_text(json.dumps(changed))
            wait_for(lambda: marker.read_bytes() != initial, output)
            run(generate() + ['--check'])
            custom.write_bytes(original)
            wait_for(lambda: marker.read_bytes() == initial, output)
        finally:
            watch.terminate()
            try:
                watch.wait(timeout=10)
            except subprocess.TimeoutExpired:
                watch.kill()
                watch.wait()
    assert artifact.read_bytes() == custom.read_bytes() == original
    # Restore the standard path so the first rename exercises input relocation.
    manifest.write_bytes(original_manifest)
    run(generate() + ['--check'])
    print('PASS imported AsyncAPI input preservation, all-service staging, configured watch and invalid-edit recovery')
else:
    artifact = Path('.forklaunch/asyncapi/realtime/asyncapi.json')
    original = artifact.read_bytes()
    config = Path('src/client-sdk/typescript-websocket/realtime/tsconfig.json').read_bytes()
    edited = Path('src/client-sdk/rust/realtime-websocket-sdk/socket_chat.rs')
    edited.write_bytes(edited.read_bytes() + b'\n// preserved user edits\n')
    run(['change', 'service', '--path', 'src/realtime', '-N', 'live-events', '--confirm'])
    assert Path('.forklaunch/asyncapi/live-events/asyncapi.json').read_bytes() == original
    assert not artifact.exists()
    assert edited.read_bytes().endswith(b'// preserved user edits\n')
    assert Path('src/client-sdk/typescript-websocket/live-events/tsconfig.json').read_bytes() == config
    run(generate('live-events') + ['--check'])
    # A custom source survives rename and explicit project deletion unchanged.
    manifest.write_text(manifest.read_text().replace('[projects.metadata]\n', '[projects.metadata]\nasyncapiArtifactPath = "custom/server-asyncapi.json"\n'))
    run(generate('live-events'))
    run(['change', 'service', '--path', 'src/live-events', '-N', 'stream-events', '--confirm'])
    run(generate('stream-events') + ['--check'])
    assert Path('custom/server-asyncapi.json').read_bytes() == original
    project = next(p for p in tomllib.loads(manifest.read_text())['projects'] if p['name'] == 'stream-events')
    assert project['metadata']['asyncapiArtifactPath'] == 'custom/server-asyncapi.json'
    old = Path('src/client-sdk/typescript-websocket/realtime/node_modules').resolve()
    Path('src/client-sdk/typescript-websocket/stream-events/node_modules').symlink_to(old, target_is_directory=True)
    subprocess.run([os.environ['TSC_BIN'], '--project', 'src/client-sdk/typescript-websocket/stream-events/tsconfig.json'], check=True, timeout=120)
    subprocess.run(['cargo', 'rustc', '-p', 'stream-events-websocket-sdk', '--lib', '--', '-D', 'warnings'], check=True, timeout=300)
    print('PASS imported SDK service rename, standard/custom document ownership, preserved edits and strict consumers')
