#!/usr/bin/env python3
"""Byte-preserving static captures in neutral, native and mixed workspaces."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-storefront'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
account = 'parity-offline-storefront-' + root.name.removeprefix('run.').replace('_', '-')
account_path = Path.home() / '.forklaunch/accounts' / (account + '.toml')
assert not account_path.exists()
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'FORKLAUNCH_ACCOUNT': account}
# No Cargo output is built here. Freeze the CLI independently of concurrent
# compilation so the complete mutation proof refers to a single executable.
shutil.copy2(binary, root / 'forklaunch')
binary = root / 'forklaunch'
cli_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
print('CLI SHA256:', cli_hash, flush=True)
capture = root / 'capture'
(capture / 'site/assets').mkdir(parents=True)
(capture / 'site/Cargo.toml').write_text('# A captured asset, not a Cargo project\n')
(capture / 'site/index.html').write_text('<img src="assets/image.bin">')
(capture / 'site/assets/image.bin').write_bytes(bytes(range(256)))
(capture / 'manifest.json').write_text(json.dumps({'schemaVersion': 1, 'source': {'domain': 'store.example'}, 'pages': [{'route': '/', 'type': 'home'}], 'limits': [], 'catalog': {'productCount': 0, 'variantCount': 0}}))

def snapshot(directory):
    return {str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in directory.rglob('*') if p.is_file() and 'node_modules' not in p.parts}

results = []
for case, runtime, modules, native in [
    ('native-default', 'node', 'src', True),
    ('node-custom', 'node', 'modules', False),
    ('bun-default', 'bun', 'src', False),
    ('legacy-root', 'node', 'crates', True),
]:
    app = root / case
    def cli(*args, path=None, success=True):
        result = subprocess.run([str(binary), *args, '--path', str(path or app)], cwd=root, env=env,
                                capture_output=True, text=True, timeout=180)
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        return result
    args = ['init', 'application', 'capture-app', '--database', 'postgresql', '--validator', 'zod',
            '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express',
            '--runtime', runtime, '--test-framework', 'vitest',
            '-D', 'Captured storefront', '-A', 'ForkLaunch', '-L', 'MIT']
    if modules != 'src':
        args += ['--modules-path', 'src' if modules == 'crates' else modules]
    cli(*args)
    if modules == 'crates':
        # Authored legacy manifest fixture: current init deliberately offers
        # src/, src/modules/ and modules/, while existing roots remain valid.
        manifest = app / '.forklaunch/manifest.toml'
        manifest.write_text(manifest.read_text().replace('modules_path = "src"', 'modules_path = "crates"'))
        if (app / 'src').exists():
            (app / 'src').rename(app / 'crates')
    assert tomllib.loads((app / '.forklaunch/manifest.toml').read_text())['modules_path'] == modules
    cli('init', 'library', 'native-models' if native else 'browser-models',
        '--language', 'rust' if native else 'typescript', '-D', 'Application models')
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
    if native:
        # Old captures may omit staticPath. Attaching a language must retain
        # their recorded module root, including applications that used crates/.
        manifest = app / '.forklaunch/manifest.toml'
        manifest.write_text('\n'.join(line for line in manifest.read_text().splitlines() if not line.startswith('staticPath =')) + '\n')
        cli('init', 'library', 'helpers', '--language', 'typescript', '-D', 'Mixed tooling')
        assert tomllib.loads(manifest.read_text())['modules_path'] == modules
        assert snapshot(app / modules / 'boutique') == snapshot(capture / 'site')
        cli('sync', 'library', 'boutique')
        cli('sync', 'all', '--confirm')
        # Sync validates legacy inferred locations without rewriting captures.
        # An actual metadata edit records the inferred path explicitly.
        cli('change', 'library', '-D', 'Retained capture', '--confirm', path=app / modules / 'boutique')
        data = tomllib.loads(manifest.read_text())
        storefront = next(project for project in data['projects'] if project['name'] == 'boutique')
        assert storefront['metadata']['staticPath'] == modules + '/boutique'
        assert modules + '/boutique' not in tomllib.loads((app / 'Cargo.toml').read_text())['workspace']['members']
        assert data['language'] == data['applicationLanguage'] == 'mixed'
        assert (app / modules / 'core/native-models/Cargo.toml').is_file()
        assert (app / modules / 'helpers/package.json').is_file()
        assert snapshot(app / modules / 'boutique') == snapshot(capture / 'site')
    cli('delete', 'library', 'boutique', '--continue')
    assert not (app / modules / 'boutique').exists()
    data = tomllib.loads((app / '.forklaunch/manifest.toml').read_text())
    assert not any(project['name'] in ['shop', 'boutique'] for project in data['projects'])
    assert all('boutique' not in names for names in data['project_peer_topology'].values())
    print(f'PASS {case} static capture, read-only previews, byte-preserving rename, sync, deletion and workspace preservation', flush=True)
    results.append({'case': case, 'runtime': runtime, 'modulesPath': modules, 'passed': True})
receipt = {'schemaVersion': 1, 'cliSha256': cli_hash, 'cases': results,
           'scope': 'Raw neutral app initialization; language-scoped libraries; byte-identical binary assets; dry-run purity; sync, rename and delete; native-to-mixed attachment with missing legacy staticPath and preserved module root',
           'compiled': False, 'scriptSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
if os.environ.get('FORKLAUNCH_STOREFRONT_RECEIPT'):
    Path(os.environ['FORKLAUNCH_STOREFRONT_RECEIPT']).write_text(json.dumps(receipt, indent=2) + '\n')
print(f'PASS storefront lifecycle: {root}', flush=True)
shutil.rmtree(root)
account_path.unlink(missing_ok=True)
