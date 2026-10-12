#!/usr/bin/env python3
"""Actual CLI lifecycle routing for registered Rust and TypeScript projects."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-mixed-lifecycle'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
app = root / 'app'
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'CARGO_INCREMENTAL': '0'}
mixed_fields = None


def run(*args, path=None):
    subprocess.run([str(binary), *args, '--path', str(path or app)], cwd=root, env=env, check=True, timeout=180)
    if mixed_fields is not None:
        actual = manifest()
        assert all(actual.get(key) == value for key, value in mixed_fields.items()), 'Lifecycle erased mixed application metadata'


def manifest():
    return tomllib.loads((app / '.forklaunch/manifest.toml').read_text())


run('init', 'application', 'mixed-lifecycle', '--language', 'typescript', '--modules-path', 'modules',
    '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'eslint',
    '--http-framework', 'express', '--runtime', 'node', '--test-framework', 'vitest',
    '-D', 'Mixed lifecycle', '-A', 'ForkLaunch', '-L', 'MIT')
for kind, name, extra in [('service', 'typescript-api', ['-d', 'postgresql']),
                          ('worker', 'typescript-jobs', ['-t', 'database', '-d', 'postgresql']),
                          ('library', 'typescript-models', [])]:
    run('init', kind, name, '--language', 'typescript', '-D', name, *extra)
# Existing TS apps can register a separate Cargo workspace; automatic migration
# scaffolding is exercised separately from this per-project dispatch fixture.
path = app / '.forklaunch/manifest.toml'
mixed_fields = {'language': 'mixed', 'applicationLanguage': 'mixed', 'sdkTargets': ['rust', 'typescript'],
    'cargoWorkspace': 'Cargo.toml', 'contractArtifactPath': '.forklaunch/contracts',
    'openapiPath': '.forklaunch/openapi', 'asyncapiPath': '.forklaunch/asyncapi'}
path.write_text(''.join(f'{key} = {json.dumps(value)}\n' for key, value in mixed_fields.items()) + path.read_text())
mixed_fields['app_name'] = 'mixed-lifecycle'
(app / 'Cargo.toml').write_text('''[workspace]
resolver = "3"
members = []
exclude = [".forklaunch/framework-rs", ".forklaunch/blueprint-rs"]
[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
authors = ["ForkLaunch"]
''')
for kind, name, extra in [('service', 'native-api', []), ('worker', 'native-jobs', ['-t', 'redis']),
                          ('library', 'native-models', [])]:
    run('init', kind, name, '--language', 'rust', '-D', name, *extra)
for kind, name in [('service', 'typescript-api'), ('worker', 'typescript-jobs'), ('library', 'typescript-models')]:
    before = {project['name']: project for project in manifest()['projects']}
    source = {str(p.relative_to(app)): p.read_bytes() for p in (app / 'modules' / name).rglob('*') if p.is_file()}
    run('sync', kind, name)
    after = {project['name']: project for project in manifest()['projects']}
    assert name in after, f'Mixed sync removed the TypeScript {kind} registration'
    assert before == after, f'Mixed sync unexpectedly changed project registrations for {name}'
    assert source == {str(p.relative_to(app)): p.read_bytes() for p in (app / 'modules' / name).rglob('*') if p.is_file()}
    assert not (app / 'crates' / name).exists()
print('PASS mixed sync routes each TypeScript project without native cleanup', flush=True)
for kind, name in [('service', 'native-api'), ('worker', 'native-jobs'), ('library', 'native-models')]:
    run('sync', kind, name)
# Keep package-cache cleanup inside the fixture boundary during rename commands.
package = app / 'modules/package.json'
package_data = json.loads(package.read_text())
package_data['scripts']['clean:purge'] = "node -e \"require('fs').appendFileSync('.lifecycle-clean','clean;')\""
package.write_text(json.dumps(package_data))
for kind, name in [('service', 'typescript-api'), ('worker', 'typescript-jobs'), ('library', 'typescript-models')]:
    run('change', kind, '-N', name + '-renamed', '--confirm', path=app / 'modules' / name)
    assert (app / 'modules' / (name + '-renamed')).is_dir()
    assert name + '-renamed' in {project['name'] for project in manifest()['projects']}
    run('change', kind, '-N', name, '--confirm', path=app / 'modules' / (name + '-renamed'))
assert (app / 'modules/.lifecycle-clean').read_text().count('clean;') == 6
print('PASS mixed TypeScript service, worker and library rename roundtrips', flush=True)
# A router operation also resolves the owning project's language.
run('init', 'router', 'profiles', '--language', 'typescript', path=app / 'modules/typescript-api')
assert manifest()['language'] == 'mixed'
server = app / 'modules/typescript-api/server.ts'
original = server.read_text()
assert 'app.use(profilesRouter)' in original
server.write_text(original.replace('app.use(profilesRouter);', ''))
owned = app / 'modules/typescript-api'
before = {str(p.relative_to(app)): p.read_bytes() for p in owned.rglob('*') if p.is_file()}
manifest_before = (app / '.forklaunch/manifest.toml').read_bytes()
failed = subprocess.run([str(binary), 'delete', 'router', 'profiles', '--continue', '--path', str(owned)],
    cwd=root, env=env, capture_output=True, text=True, timeout=30)
assert failed.returncode != 0 and 'Failed to delete from server.ts' in failed.stderr
assert before == {str(p.relative_to(app)): p.read_bytes() for p in owned.rglob('*') if p.is_file()}
assert manifest_before == (app / '.forklaunch/manifest.toml').read_bytes()
server.write_text(original)
run('delete', 'router', 'profiles', '--continue', path=app / 'modules/typescript-api')
assert 'profilesRouter' not in server.read_text()
assert not (owned / 'api/routes/profiles.routes.ts').exists()
print('PASS async TypeScript router removal and preservation on unsupported server edits', flush=True)
for kind, name in [('service', 'typescript-api'), ('worker', 'typescript-jobs'), ('library', 'typescript-models')]:
    run('delete', kind, name, '--continue')
    assert not (app / 'modules' / name).exists()
    assert name not in {project['name'] for project in manifest()['projects']}
    assert manifest()['language'] == 'mixed'
    assert manifest()['applicationLanguage'] == 'mixed'
    for native in ['native-api', 'native-jobs', 'native-models']:
        assert (app / 'crates' / native / 'Cargo.toml').exists()
for kind, name in [('service', 'native-api'), ('worker', 'native-jobs'), ('library', 'native-models')]:
    run('delete', kind, name, '--continue')
    assert not (app / 'crates' / name).exists()
assert manifest()['runtime'] == 'node'
print('PASS actual mixed TypeScript/Rust service, worker, library and router lifecycle dispatch', flush=True)
print(f'Mixed lifecycle fixture evidence: {root}', flush=True)
