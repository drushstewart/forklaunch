#!/usr/bin/env python3
"""Rust-first workspace attachment through actual CLI project initialization."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-typescript-attach'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
app = root / 'app'
account = 'parity-offline-' + root.name.removeprefix('run.').replace('_', '-')
account_path = Path.home() / '.forklaunch/accounts' / f'{account}.toml'
assert not account_path.exists()
env = {**os.environ, 'FORKLAUNCH_ACCOUNT': account, 'FORKLAUNCH_SKIP_FORMAT': '1', 'CARGO_INCREMENTAL': '0'}


def run(*args, success=True):
    result = subprocess.run([str(binary), *args, '--path', str(app)], cwd=root, env=env,
        capture_output=True, text=True, timeout=300)
    print(result.stdout, end='', flush=True)
    if success:
        assert result.returncode == 0, result.stderr
    else:
        assert result.returncode != 0, result.stdout
    return result


def snapshot():
    return {str(path.relative_to(app)): ('link:' + os.readlink(path) if path.is_symlink()
        else hashlib.sha256(path.read_bytes()).hexdigest()) for path in app.rglob('*')
        if path.is_symlink() or path.is_file()}


run('init', 'application', 'rust-first', '--language', 'rust', '-D', 'Rust-first attachment', '-A', 'ForkLaunch', '-L', 'MIT')
run('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
run('init', 'library', 'native-models', '--language', 'rust', '-D', 'Native models')
source = app / 'crates/native-api/src/lib.rs'
source.write_text(source.read_text() + '\npub const APPLICATION_OWNED: bool = true;\n')
original_source = source.read_bytes()
original_cargo = (app / 'Cargo.toml').read_bytes()
original_dockerfile = (app / 'Dockerfile').read_bytes()
package = json.loads((app / 'package.json').read_text())
package['scripts']['user:owned'] = 'echo preserved'
(app / 'package.json').write_text(json.dumps(package))
before = snapshot()
run('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API', '--dryrun')
assert snapshot() == before, 'Dry run changed the application'
failed = run('init', 'service', 'rust-first', '--language', 'typescript', '-d', 'postgresql', '-D', 'Invalid', success=False)
assert snapshot() == before, 'Failed project initialization changed the application'
run('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API')
manifest = tomllib.loads((app / '.forklaunch/manifest.toml').read_text())
assert manifest['language'] == manifest['applicationLanguage'] == 'mixed'
assert manifest['runtime'] == 'node' and manifest['modules_path'] == 'modules'
assert manifest['sdkTargets'] == ['typescript', 'rust']
assert manifest['cargoWorkspace'] == 'Cargo.toml'
assert {p['name'] for p in manifest['projects']} >= {'native-api', 'native-models', 'core', 'monitoring', 'client-sdk', 'legacy-api'}
assert source.read_bytes() == original_source
assert (app / 'Cargo.toml').read_bytes() == original_cargo
assert (app / 'Dockerfile').read_bytes() == original_dockerfile
assert (app / 'Dockerfile.rust').read_bytes() == original_dockerfile
scripts = json.loads((app / 'package.json').read_text())['scripts']
assert scripts['user:owned'] == 'echo preserved'
assert scripts['lint'] == 'pnpm --dir modules run lint'
assert scripts['build'] == 'cargo build --workspace && pnpm -r --no-bail run build'
assert 'sdks/typescript' in (app / 'pnpm-workspace.yaml').read_text()
assert 'modules/*' in (app / 'pnpm-workspace.yaml').read_text()
for path in app.rglob('*'):
    if path.is_file() and not path.is_symlink():
        assert b'/template/' not in path.read_bytes() and b'/app/modules' not in path.read_bytes(), path
run('init', 'library', 'legacy-models', '--language', 'typescript', '-D', 'Legacy models')
run('init', 'worker', 'legacy-jobs', '--language', 'typescript', '-t', 'redis', '-d', 'postgresql', '-D', 'Legacy jobs')
run('init', 'module', 'legacy-cac', '--language', 'typescript', '-m', 'cac-base', '-d', 'postgresql')
compose = json.loads(subprocess.check_output(['docker', 'compose', '-f', str(app / 'docker-compose.yaml'),
    'config', '--no-interpolate', '--format', 'json'], cwd=app, env=env, text=True))
assert compose['services']['native-api']['command'] == ['./target/release/native-api']
assert compose['services']['legacy-api']['build']['context'] == str(app / 'modules')
listed = json.loads(subprocess.check_output(['pnpm', '-r', 'list', '--depth', '-1', '--json'], cwd=app, env=env, text=True))
assert {'@forklaunch-generated/sdk', '@rust-first/legacy-api', '@rust-first/legacy-models'} <= {p['name'] for p in listed}
subprocess.run(['pnpm', 'install', '--ignore-scripts', '--no-frozen-lockfile'], cwd=app, env=env, check=True, timeout=300)
subprocess.run(['pnpm', 'build'], cwd=app, env=env, check=True, timeout=1800)
subprocess.run(['node', str(repo / 'cli/tests/rust_typescript_service_startup.mjs'), str(app)],
    cwd=app, env=env, check=True, timeout=60)
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=app, env=env))
subprocess.run(['node', str(repo / 'cli/tests/rust_mcp_service_startup.mjs'),
    str(Path(metadata['target_directory']) / 'debug/native-api')], cwd=app, env=env, check=True, timeout=120)
print('PASS Rust-first TypeScript workspace attachment, dry run/failure preservation, project kinds, Cargo and HTTP/MCP startup', flush=True)
# Every supported project kind can be the first TypeScript addition.
for kind, options in [
    ('library', ['-D', 'First library']),
    ('worker', ['-t', 'redis', '-d', 'postgresql', '-D', 'First worker']),
    ('module', ['-m', 'cac-base', '-d', 'postgresql']),
]:
    app = root / (kind + '-first')
    run('init', 'application', 'rust-first', '--language', 'rust', '--sdk-targets', 'rust',
        '-D', 'Rust-only SDK selection', '-A', 'ForkLaunch', '-L', 'MIT')
    run('init', kind, 'legacy-first', '--language', 'typescript', *options)
    manifest = tomllib.loads((app / '.forklaunch/manifest.toml').read_text())
    assert manifest['sdkTargets'] == ['rust']
    assert manifest['runtime'] == 'node'
    project_name = 'cac' if kind == 'module' else 'legacy-first'
    assert any(p['name'] == project_name for p in manifest['projects'])
    assert not (app / 'sdks/typescript').exists()
    assert (app / f'modules/{project_name}/package.json').is_file()
    run('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API after attachment')
    assert (app / 'Dockerfile.rust').is_file()
    print(f'PASS first TypeScript {kind} attachment and subsequent native initialization', flush=True)
app = root / 'conflict'
run('init', 'application', 'rust-first', '--language', 'rust', '--sdk-targets', 'rust',
    '-D', 'Reserved project collision', '-A', 'ForkLaunch', '-L', 'MIT')
run('init', 'library', 'core', '--language', 'rust', '-D', 'User native core')
before = snapshot()
failed = run('init', 'library', 'legacy-first', '--language', 'typescript', '-D', 'Conflicting workspace', success=False)
assert 'already registered' in failed.stderr
assert snapshot() == before
print('PASS foundational project name conflict rejects without changing the native application', flush=True)
assert not account_path.exists(), 'Local scaffolding unexpectedly created login credentials'
print(f'Rust-first attachment fixture evidence: {root}', flush=True)
