#!/usr/bin/env python3
"""Attach native backends to actual Node/Bun scaffolds and start HTTP/MCP."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-mixed-attach'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'CARGO_INCREMENTAL': '0'}

for runtime in ['node', 'bun']:
    app = root / runtime
    def run(*args):
        subprocess.run([str(binary), *args, '--path', str(app)], cwd=root, env=env, check=True, timeout=180)
    def snapshot():
        return {str(path.relative_to(app)): path.read_bytes() for path in (app / 'modules').rglob('*') if path.is_file()}
    run('init', 'application', runtime + '-mixed', '--language', 'typescript', '--modules-path', 'modules',
        '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'eslint',
        '--http-framework', 'express', '--runtime', runtime, '--test-framework', 'vitest',
        '-D', 'Attach native backend', '-A', 'ForkLaunch', '-L', 'MIT')
    run('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API')
    original = snapshot()
    manifest_path = app / '.forklaunch/manifest.toml'
    original_manifest = manifest_path.read_bytes()
    (app / 'Dockerfile').write_text('# Existing custom TypeScript image\nFROM node:22\n')
    run('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API', '--dryrun')
    assert not (app / 'Cargo.toml').exists()
    assert not (app / 'crates/native-api').exists()
    assert manifest_path.read_bytes() == original_manifest
    assert snapshot() == original
    run('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
    run('init', 'worker', 'native-jobs', '--language', 'rust', '-t', 'redis', '-D', 'Native jobs')
    run('init', 'library', 'native-models', '--language', 'rust', '-D', 'Native models')
    run('init', 'module', 'native-cac', '--language', 'rust', '-m', 'cac-base')
    assert snapshot() == original, 'Native attachment rewrote TypeScript source or tooling'
    assert (app / 'Dockerfile').read_text() == '# Existing custom TypeScript image\nFROM node:22\n'
    assert 'cargo build --release --workspace --bins' in (app / 'Dockerfile.rust').read_text()
    assert '**/node_modules/' in (app / 'Dockerfile.rust.dockerignore').read_text()
    manifest = tomllib.loads(manifest_path.read_text())
    assert manifest['runtime'] == runtime and manifest['modules_path'] == 'modules'
    assert manifest['language'] == manifest['applicationLanguage'] == 'mixed'
    assert manifest['cargoWorkspace'] == 'Cargo.toml'
    assert manifest['sdkTargets'] == ['typescript', 'rust']
    assert {p['name'] for p in manifest['projects']} >= {'legacy-api', 'native-api', 'native-jobs', 'native-models', 'native-cac'}
    compose = json.loads(subprocess.check_output(['docker', 'compose', '-f', str(app / 'docker-compose.yaml'),
        'config', '--no-interpolate', '--format', 'json'], cwd=app, env=env, text=True))
    for service in ['native-api', 'native-jobs', 'native-cac']:
        assert compose['services'][service]['build']['dockerfile'] == './Dockerfile.rust'
        assert compose['services'][service]['command'] == ['./target/release/' + service]
    assert compose['services']['legacy-api']['build']['dockerfile'] == './Dockerfile'
    # Reinitialization must retain existing code and workspace configuration.
    source = app / 'crates/native-api/src/lib.rs'
    source.write_text(source.read_text() + '\npub const APPLICATION_OWNED: bool = true;\n')
    run('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
    assert 'APPLICATION_OWNED' in source.read_text()
    relocated = root / (runtime + '-relocated')
    app.rename(relocated)
    app = relocated
    subprocess.run(['cargo', 'check', '--workspace'], cwd=app, env=env, check=True, timeout=1800)
    subprocess.run(['cargo', 'build', '-p', 'native-api'], cwd=app, env=env, check=True, timeout=1800)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=app, env=env))
    subprocess.run(['node', str(repo / 'cli/tests/rust_mcp_service_startup.mjs'),
        str(Path(metadata['target_directory']) / 'debug/native-api')], cwd=app, env=env, check=True, timeout=120)
    assert snapshot() == original
    if runtime == 'node':
        subprocess.run(['bash', str(repo / 'cli/tests/rust_container_runtime.sh')], cwd=app,
            env={**env, 'FORKLAUNCH_RUST_CONTAINER_APP': str(app),
                 'FORKLAUNCH_RUST_CONTAINER_SERVICE': 'native-api',
                 'FORKLAUNCH_RUST_CONTAINER_DOCKERFILE': 'Dockerfile.rust'}, check=True, timeout=1800)
    print(f'PASS {runtime} app native service/worker/library/module attachment, relocation, preserved TS sources and HTTP/MCP startup', flush=True)
print(f'Mixed attachment fixture evidence: {root}', flush=True)
