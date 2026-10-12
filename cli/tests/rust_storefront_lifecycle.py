#!/usr/bin/env python3
"""Static capture lifecycle in Rust, Node, Bun and a Rust-to-mixed migration."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-storefront'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'FORKLAUNCH_ACCOUNT': 'parity-offline-storefront'}
capture = root / 'capture'
(capture / 'site/assets').mkdir(parents=True)
(capture / 'site/Cargo.toml').write_text('# A captured asset, not a Cargo project\n')
(capture / 'site/index.html').write_text('<img src="assets/image.bin">')
(capture / 'site/assets/image.bin').write_bytes(bytes(range(256)))
(capture / 'manifest.json').write_text(json.dumps({'schemaVersion': 1, 'source': {'domain': 'store.example'}, 'pages': [{'route': '/', 'type': 'home'}], 'limits': [], 'catalog': {'productCount': 0, 'variantCount': 0}}))

def snapshot(directory):
    return {str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in directory.rglob('*') if p.is_file() and 'node_modules' not in p.parts}

for runtime in ['rust', 'node', 'bun']:
    app = root / runtime
    def cli(*args, path=None, success=True):
        result = subprocess.run([str(binary), *args, '--path', str(path or app)], cwd=root, env=env,
                                capture_output=True, text=True, timeout=180)
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        return result
    args = ['init', 'application', 'capture-app', '--language', 'rust' if runtime == 'rust' else 'typescript',
            '-D', 'Captured storefront', '-A', 'ForkLaunch', '-L', 'MIT']
    if runtime != 'rust':
        args += ['--modules-path', 'modules', '--database', 'postgresql', '--validator', 'zod',
                 '--formatter', 'prettier', '--linter', 'eslint', '--http-framework', 'express',
                 '--runtime', runtime, '--test-framework', 'vitest']
    cli(*args)
    modules = 'crates' if runtime == 'rust' else 'modules'
    original = snapshot(app)
    cli('init', 'module', 'storefront', 'shop', '--from', str(capture / 'manifest.json'), '--dryrun')
    assert snapshot(app) == original, 'Capture dry run mutated the app'
    cli('init', 'module', 'storefront', 'shop', '--from', str(capture / 'manifest.json'))
    assert snapshot(app / modules / 'shop') == snapshot(capture / 'site')
    for path, digest in original.items():
        if path != '.forklaunch/manifest.toml':
            assert hashlib.sha256((app / path).read_bytes()).hexdigest() == digest, path
    cli('sync', 'library', 'shop')
    before = snapshot(app)
    cli('change', 'library', '-N', 'boutique', '-D', 'Renamed capture', '--dryrun', path=app / modules / 'shop')
    assert snapshot(app) == before, 'Rename dry run mutated the app'
    cli('change', 'library', '-N', 'boutique', '-D', 'Renamed capture', '--confirm', path=app / modules / 'shop')
    assert snapshot(app / modules / 'boutique') == snapshot(capture / 'site')
    assert not (app / modules / 'shop').exists()
    if runtime == 'rust':
        # Also cover old captures without explicit path metadata during migration.
        manifest = app / '.forklaunch/manifest.toml'
        manifest.write_text('\n'.join(line for line in manifest.read_text().splitlines() if not line.startswith('staticPath =')) + '\n')
        cli('init', 'library', 'helpers', '--language', 'typescript', '-D', 'Mixed tooling')
        assert tomllib.loads(manifest.read_text())['modules_path'] == 'modules'
        assert snapshot(app / 'crates/boutique') == snapshot(capture / 'site')
        cli('sync', 'library', 'boutique')
        cli('sync', 'all', '--confirm')
        data = tomllib.loads(manifest.read_text())
        storefront = next(project for project in data['projects'] if project['name'] == 'boutique')
        assert storefront['metadata']['staticPath'] == 'crates/boutique'
        assert 'crates/boutique' not in tomllib.loads((app / 'Cargo.toml').read_text())['workspace']['members']
    cli('delete', 'library', 'boutique', '--continue')
    assert not (app / modules / 'boutique').exists()
    data = tomllib.loads((app / '.forklaunch/manifest.toml').read_text())
    assert not any(project['name'] in ['shop', 'boutique'] for project in data['projects'])
    assert all('boutique' not in names for names in data['project_peer_topology'].values())
    print(f'PASS {runtime} static capture, read-only previews, byte-preserving rename, sync, deletion and workspace preservation', flush=True)
print(f'PASS storefront lifecycle: {root}', flush=True)
