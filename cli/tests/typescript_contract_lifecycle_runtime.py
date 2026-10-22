#!/usr/bin/env python3
"""Exercise generated hook ownership through real CLI rename/delete commands."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

REPO = Path(__file__).resolve().parents[2]
CLI = REPO / 'cli/target/debug/forklaunch'
ROOT = Path(tempfile.mkdtemp(prefix='forklaunch-contract-lifecycle-'))
APP = ROOT / 'app'

def run(*args, path=APP, check=True):
    result = subprocess.run([str(CLI), *args, '--path', str(path)], cwd=ROOT,
                            env={**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1'}, text=True, capture_output=True)
    if check and result.returncode:
        raise RuntimeError(result.stdout[-4000:] + result.stderr[-6000:])
    return result

try:
    run('init', 'application', 'lifecycle-proof', '--language', 'typescript', '--modules-path', 'modules',
        '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'oxlint',
        '--http-framework', 'express', '--runtime', 'node', '--test-framework', 'vitest',
        '-D', 'Lifecycle proof', '-A', 'ForkLaunch', '-L', 'MIT')
    for kind, name in [('service', 'records'), ('worker', 'jobs')]:
        extra = ['-t', 'database'] if kind == 'worker' else []
        run('init', kind, name, '--language', 'typescript', '-d', 'postgresql', '-D', name, *extra)
        config = APP / '.forklaunch/build.json'
        original = config.read_text()
        manifest = (APP / '.forklaunch/manifest.toml').read_bytes()
        customized = json.loads(original)
        customized['contracts'][name]['customized'] = True
        config.write_text(json.dumps(customized))
        rejected = run('change', kind, '-N', name + '-next', '-c', path=APP / f'modules/{name}', check=False)
        assert rejected.returncode and 'customized' in rejected.stderr.lower(), rejected.stderr
        assert (APP / '.forklaunch/manifest.toml').read_bytes() == manifest
        assert (APP / f'modules/{name}').is_dir()
        config.write_text(original)
        old = json.loads(original)['contracts'][name]
        ts_neighbor = APP / old['typescript']
        ts_neighbor = ts_neighbor.parent / 'user-owned.ts'
        ts_neighbor.write_text('export const keep = true;\n')
        rust_neighbor = (APP / old['rust']).parent / 'user_owned.rs'
        rust_neighbor.write_text('// keep application-owned source\n')
        run('change', kind, '-N', name + '-next', '-c', path=APP / f'modules/{name}')
        new = name + '-next'
        renamed = json.loads(config.read_text())['contracts'][new]
        assert renamed['service'] == f'modules/{new}'
        assert name not in json.loads(config.read_text())['contracts']
        assert not (APP / old['typescript']).exists()
        assert not (APP / old['rust']).exists()
        assert ts_neighbor.is_file() and rust_neighbor.is_file()
        cargo = tomllib.loads((APP / 'Cargo.toml').read_text())
        assert f'sdks/rust/{name}-sdk' not in cargo['workspace']['members']
        assert f'sdks/rust/{new}-sdk' in cargo['workspace']['members']
        package = json.loads((APP / f'modules/{new}/package.json').read_text())
        assert package['scripts']['build'].endswith(' ' + new)
        assert (APP / renamed['typescript']).is_file() and (APP / renamed['rust']).is_file()
        assert not (APP / renamed['contract']).exists()
        run('delete', kind, new, '-c')
        assert new not in json.loads(config.read_text()).get('contracts', {})
        assert not (APP / renamed['typescript']).exists() and not (APP / renamed['rust']).exists()
        assert ts_neighbor.is_file() and rust_neighbor.is_file()
        assert f'sdks/rust/{new}-sdk' not in tomllib.loads((APP / 'Cargo.toml').read_text())['workspace']['members']
        print(f'PASS {kind} rename/delete maintains hook/SDK ownership and preserves neighboring user files', flush=True)
finally:
    if os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        print('Fixture retained:', ROOT, flush=True)
    else:
        shutil.rmtree(ROOT)
