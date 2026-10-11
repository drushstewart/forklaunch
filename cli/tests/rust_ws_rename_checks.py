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
source = Path('crates/realtime/src/lib.rs')
original_source = source.read_bytes()
old_rust = Path('sdks/rust/realtime-websocket-sdk')
old_ts = Path('sdks/typescript-websocket/realtime')
edited = old_rust / 'src/socket_chat.rs'
edited.write_bytes(edited.read_bytes() + b'\n// user-owned old SDK edits\n')
manual = old_ts / 'src/user.ts'
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
    run([cli, 'change', 'service', '--path', f'crates/{old}', '-N', new, '--confirm'])
    assert not Path(f'crates/{old}').exists()
    assert Path(f'crates/{new}/src/socket_contract_fixture.json').exists()
    project = next(project for project in tomllib.loads(manifest.read_text())['projects'] if project['name'] == new)
    artifact = Path(project['metadata'].get('socketContractArtifactPath', f'.forklaunch/socket-contracts/{new}/socket-contract.json'))
    portable = json.loads(artifact.read_text())
    assert portable['title'] == new
    marker = json.loads(Path(f'.forklaunch/socket-contracts/{new}/registry-export.json').read_text())
    assert marker['outputPath'] == str(artifact)
    assert marker['sha256'] == hashlib.sha256(artifact.read_bytes()).hexdigest()
    cargo = tomllib.loads(Path('Cargo.toml').read_text())
    assert f'sdks/rust/{old}-websocket-sdk' not in cargo['workspace']['members']
    assert f'sdks/rust/{new}-websocket-sdk' in cargo['workspace']['members']
    assert json.loads(Path(f'sdks/typescript-websocket/{new}/tsconfig.json').read_text()) == config
    assert json.loads(Path(f'sdks/typescript-websocket/{new}/package.json').read_text())['name'] == f'{new}-websocket-sdk'
    assert tomllib.loads(Path(f'sdks/rust/{new}-websocket-sdk/Cargo.toml').read_text())['package']['name'] == f'{new}-websocket-sdk'
    assert not Path(f'.forklaunch/socket-sdk-outputs/{old}').exists()
    run(generate(new) + ['--check'])


# Occupied generated outputs reject the rename before the source/manifest moves.
occupied = Path('sdks/typescript-websocket/live-events')
occupied.mkdir()
(occupied / 'user.txt').write_text('preserved')
before = manifest.read_bytes()
run([cli, 'change', 'service', '--path', 'crates/realtime', '-N', 'live-events', '--confirm'], failure='already exists')
assert manifest.read_bytes() == before and source.read_bytes() == original_source
assert (occupied / 'user.txt').read_text() == 'preserved'
(occupied / 'user.txt').unlink()
occupied.rmdir()
# An explicitly configured default path must follow the service name.
manifest.write_text(manifest.read_text().replace('[projects.metadata]\n', '[projects.metadata]\nsocketContractArtifactPath = ".forklaunch/socket-contracts/realtime/socket-contract.json"\n'))
rename('realtime', 'live-events')
assert edited.read_bytes().endswith(b'// user-owned old SDK edits\n')
assert manual.read_text() == '// user-owned old SDK source\n'
# A custom artifact path remains custom, while its registry service title changes.
manifest.write_text(manifest.read_text().replace('.forklaunch/socket-contracts/live-events/socket-contract.json', 'custom/renamed-sockets.json'))
run([cli, 'openapi', 'export', '--path', '.'])
rename('live-events', 'stream-events')
assert Path('custom/renamed-sockets.json').exists()
assert not Path('.forklaunch/socket-contracts/live-events').exists()
assert Path('crates/stream-events/src/lib.rs').read_text().count('register_socket') == 1
assert 'pub const SERVICE_NAME: &str = "stream-events";' in Path('crates/stream-events/src/lib.rs').read_text()
# Strict consumers remain buildable after both renames.
new_ts = Path('sdks/typescript-websocket/stream-events')
(new_ts / 'node_modules').symlink_to((old_ts / 'node_modules').resolve(), target_is_directory=True)
run([os.environ['TSC_BIN'], '--project', str(new_ts / 'tsconfig.json')])
run(['cargo', 'rustc', '-p', 'stream-events-websocket-sdk', '--lib', '--', '-D', 'warnings'])
run(['cargo', 'build', '-p', 'stream-events'])
print('PASS registered socket service renames, default/custom artifacts, package/workspace names, compiler options, preserved old user files, collision rejection and strict consumers')
