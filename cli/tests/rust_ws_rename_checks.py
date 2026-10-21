"""Check registered socket/default/custom artifact and SDK service rename."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib

cli = sys.argv[1]
manifest = Path('.forklaunch/manifest.toml')
source = Path('src/realtime/.forklaunch/contracts/lib.rs')
original_source = source.read_bytes()
old_rust = Path('src/client-sdk/rust/realtime-websocket-sdk')
old_ts = Path('src/client-sdk/typescript-websocket/realtime')
edited = old_rust / 'socket_chat.rs'
edited.write_bytes(edited.read_bytes() + b'\n// user-owned old SDK edits\n')
manual = old_ts / 'user.ts'
manual.write_text('// user-owned old SDK source\n')
config = json.loads((old_ts / 'tsconfig.json').read_text())
config['compilerOptions']['noUnusedLocals'] = False
(old_ts / 'tsconfig.json').write_text(json.dumps(config))


def run(command, failure=None):
    result = subprocess.run(command, capture_output=True, text=True, timeout=300)
    output = result.stdout + result.stderr
    if failure is None:
        assert result.returncode == 0, output
    else:
        assert result.returncode != 0 and failure in output, output


def generate(name):
    return [cli, 'sdk', 'generate', '--path', '.', '--from', 'socket-contract', '--target', 'all', '--service', name]


def rename(old, new):
    run([cli, 'change', 'service', '--path', f'src/{old}', '-N', new, '--confirm'])
    assert not Path(f'src/{old}').exists()
    assert Path(f'src/{new}/api/controllers/socket_contract_fixture.json').exists()
    project = next(project for project in tomllib.loads(manifest.read_text())['projects'] if project['name'] == new)
    sdk = Path(project['metadata']['cratePath']) / 'sdk/typescript'
    artifact = Path(project['metadata'].get('socketContractArtifactPath', str(sdk / 'socket-contract.json')))
    portable = json.loads(artifact.read_text())
    assert portable['title'] == new
    marker = json.loads((sdk / 'socket-registry-export.json').read_text())
    assert marker['outputPath'] == str(artifact)
    assert marker['sha256'] == hashlib.sha256(artifact.read_bytes()).hexdigest()
    cargo = tomllib.loads(Path('Cargo.toml').read_text())
    assert f'src/client-sdk/rust/{old}-websocket-sdk' not in cargo['workspace']['members']
    assert f'src/client-sdk/rust/{new}-websocket-sdk' in cargo['workspace']['members']
    assert json.loads(Path(f'src/client-sdk/typescript-websocket/{new}/tsconfig.json').read_text()) == config
    assert json.loads(Path(f'src/client-sdk/typescript-websocket/{new}/package.json').read_text())['name'] == f'{new}-websocket-sdk'
    assert tomllib.loads(Path(f'src/client-sdk/rust/{new}-websocket-sdk/Cargo.toml').read_text())['package']['name'] == f'{new}-websocket-sdk'
    assert not Path(f'src/client-sdk/typescript-websocket/.{old}.forklaunch.json').exists()
    assert not Path(f'src/client-sdk/rust/.{old}-websocket-sdk.forklaunch.json').exists()
    assert sorted(str(p) for p in Path('.forklaunch').rglob('*')) == ['.forklaunch/manifest.toml']
    run(generate(new) + ['--check'])


# Occupied generated outputs reject the rename before the source/manifest moves.
occupied = Path('src/client-sdk/typescript-websocket/live-events')
occupied.mkdir()
(occupied / 'user.txt').write_text('preserved')
before = manifest.read_bytes()
run([cli, 'change', 'service', '--path', 'src/realtime', '-N', 'live-events', '--confirm'], failure='already exists')
assert manifest.read_bytes() == before and source.read_bytes() == original_source
assert (occupied / 'user.txt').read_text() == 'preserved'
(occupied / 'user.txt').unlink()
occupied.rmdir()
# An explicitly configured path inside the service must follow its crate.
manifest.write_text(manifest.read_text().replace('[projects.metadata]\n', '[projects.metadata]\nsocketContractArtifactPath = "src/realtime/sdk/typescript/socket-contract.json"\n'))
rename('realtime', 'live-events')
assert edited.read_bytes().endswith(b'// user-owned old SDK edits\n')
assert manual.read_text() == '// user-owned old SDK source\n'
# A custom artifact path remains custom, while its registry service title changes.
manifest.write_text(manifest.read_text().replace('src/live-events/sdk/typescript/socket-contract.json', 'custom/renamed-sockets.json'))
run([cli, 'openapi', 'export', '--path', '.'])
rename('live-events', 'stream-events')
assert Path('custom/renamed-sockets.json').exists()
assert not Path('src/stream-events/sdk/typescript/socket-contract.json').exists()
assert Path('src/stream-events/.forklaunch/contracts/lib.rs').read_text().count('register_socket') == 1
assert 'ServiceContract::new("stream-events"' in Path('src/stream-events/.forklaunch/contracts/lib.rs').read_text()
# Strict consumers remain buildable after both renames.
new_ts = Path('src/client-sdk/typescript-websocket/stream-events')
(new_ts / 'node_modules').symlink_to((old_ts / 'node_modules').resolve(), target_is_directory=True)
run([os.environ['TSC_BIN'], '--project', str(new_ts / 'tsconfig.json')])
run(['cargo', 'rustc', '-p', 'stream-events-websocket-sdk', '--lib', '--', '-D', 'warnings'])
run(['cargo', 'build', '-p', 'stream-events'])
print('PASS registered socket service renames, default/custom artifacts, package/workspace names, compiler options, preserved old user files, collision rejection and strict consumers')
