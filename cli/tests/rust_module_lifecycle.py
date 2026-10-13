#!/usr/bin/env python3
"""Exercise every built-in native module's lifecycle in a mixed application."""
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
cli = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'output/rust-module-lifecycle'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output))
app = root / 'app'
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'CARGO_INCREMENTAL': '0'}
variants = ['iam-base', 'iam-better-auth', 'cac-base', 'messaging-base',
            'messaging-twilio', 'ecommerce-stripe', 'billing-base', 'billing-stripe']


def command(*args, cwd=None):
    subprocess.run([str(arg) for arg in args], cwd=cwd or app, env=env, check=True, timeout=900)


def run(*args, path=None):
    command(cli, *args, '--path', path or app, cwd=root)


def check_sdks():
    command('cargo', 'check', '--workspace')
    command(os.environ.get('TSC_BIN', repo / 'framework/node_modules/.bin/tsc'),
            '--project', 'sdks/typescript/tsconfig.json', '--noEmit')


def manifest():
    return tomllib.loads((app / '.forklaunch/manifest.toml').read_text())


def project(name):
    return next(p for p in manifest()['projects'] if p['name'] == name)


run('init', 'application', 'module-matrix', '--language', 'mixed',
    '-D', 'Module lifecycle matrix', '-A', 'ForkLaunch', '-L', 'MIT')
run('init', 'library', 'shared-client', '--language', 'typescript', '-D', 'Shared client')
for variant in variants:
    run('init', 'module', variant, '--language', 'rust', '-m', variant)
app.rename(root / 'relocated')
app = root / 'relocated'
client_path = next(p for p in app.rglob('shared-client/package.json') if '.forklaunch' not in p.parts)
client_files = {p.relative_to(client_path.parent): p.read_bytes()
                for p in client_path.parent.rglob('*') if p.is_file()}

for variant in variants:
    source = app / 'crates' / variant
    with (source / 'src/lib.rs').open('a') as file:
        file.write('\npub const USER_LIFECYCLE_MARKER: &str = "preserved";\n')
    with (source / 'Cargo.toml').open('a') as file:
        file.write('\n[package.metadata.user]\nmarker = "preserved"\n')
    run('sync', 'service', variant)
    run('init', 'module', variant, '--language', 'rust', '-m', variant)
    assert 'USER_LIFECYCLE_MARKER' in (source / 'src/lib.rs').read_text()
    assert project(variant)['variant'] == variant

run('openapi', 'export')
for variant in variants:
    document = json.loads((app / '.forklaunch/openapi' / variant / 'openapi.json').read_text())
    for group in document.get('components', {}).values():
        assert all(re.fullmatch(r'[a-zA-Z0-9._-]+', name) for name in group), variant
run('sdk', 'generate', '--target', 'all', '--from', 'openapi')
check_sdks()
run('sdk', 'generate', '--target', 'all', '--from', 'contract')
for variant in variants:
    name = variant + '-renamed'
    run('change', 'service', '-N', name, '--confirm', path=app / 'crates' / variant)
    source = app / 'crates' / name
    assert not (app / 'crates' / variant).exists()
    assert project(name)['variant'] == variant
    assert 'USER_LIFECYCLE_MARKER' in (source / 'src/lib.rs').read_text()
    assert tomllib.loads((source / 'Cargo.toml').read_text())['package']['metadata']['user']['marker'] == 'preserved'
    run('sync', 'service', name)
    assert (app / 'sdks/rust' / (name + '-sdk') / 'Cargo.toml').is_file()
    assert (app / '.forklaunch/contracts' / name / 'contract.json').is_file()
    assert not (app / '.forklaunch/contracts' / variant).exists()
    print(f'PASS {variant}: init, relocation, sync, idempotency, rename, source and variant preservation', flush=True)

run('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
check_sdks()
for variant in variants:
    name = variant + '-renamed'
    run('delete', 'service', name, '--continue')
    for path in ['crates', 'sdks/rust', '.forklaunch/contracts', '.forklaunch/openapi']:
        assert not (app / path / (name + '-sdk' if path == 'sdks/rust' else name)).exists()
    assert all(p['name'] != name for p in manifest()['projects'])
    assert all(name not in peers for peers in manifest()['project_peer_topology'].values())
    print(f'PASS {variant}: delete and removal of contracts, SDKs and registrations', flush=True)
assert manifest()['language'] == 'mixed'
assert client_files == {p.relative_to(client_path.parent): p.read_bytes()
                        for p in client_path.parent.rglob('*') if p.is_file()}
command('cargo', 'metadata', '--no-deps', '--format-version', '1')
print(f'PASS all eight module variants and preservation of the TypeScript library: {root}', flush=True)
