"""Exercise generated-service registry export lifecycle through the installed CLI."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

cli = sys.argv[1]
manifest = Path('.forklaunch/manifest.toml')
fixture = Path('src/realtime/api/controllers/socket_contract_fixture.json')
artifact = Path('src/realtime/sdk/typescript/socket-contract.json')
marker = artifact.with_name('socket-registry-export.json')
custom = Path('custom/registry-sockets.json')
original_manifest = manifest.read_text()
original_fixture = fixture.read_bytes()
original_artifact = artifact.read_bytes()


def export():
    result = subprocess.run([cli, 'openapi', 'export', '--path', '.'],
                            capture_output=True, text=True, timeout=300)
    assert result.returncode == 0, result.stdout + result.stderr


try:
    ownership = json.loads(marker.read_text())
    assert ownership['sha256'] == hashlib.sha256(original_artifact).hexdigest()
    manifest.write_text(original_manifest.replace('[projects.metadata]\n',
                        '[projects.metadata]\nsocketContractArtifactPath = "custom/registry-sockets.json"\n'))
    export()
    assert custom.read_bytes() == original_artifact
    assert not artifact.exists()
    assert json.loads(marker.read_text())['outputPath'] == str(custom)
    manifest.write_text(original_manifest)
    export()
    assert artifact.read_bytes() == original_artifact
    assert not custom.exists()

    empty = json.loads(original_fixture)
    empty['sockets'] = {}
    fixture.write_text(json.dumps(empty))
    export()
    assert not artifact.exists()
    assert not marker.exists()
    # An offline/manual artifact belongs to the user and is preserved.
    artifact.write_bytes(original_artifact + b'\n')
    export()
    assert artifact.read_bytes() == original_artifact + b'\n'
    assert not marker.exists()
    fixture.write_bytes(original_fixture)
    export()
    assert artifact.read_bytes() == original_artifact
    assert marker.exists()
finally:
    restore = manifest.read_text() != original_manifest or fixture.read_bytes() != original_fixture
    manifest.write_text(original_manifest)
    fixture.write_bytes(original_fixture)
    if restore:
        export()

print('PASS generated service socket registry export: custom path moves, last-socket removal, content ownership and manual artifact preservation')
