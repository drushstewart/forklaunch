"""Better Auth wrapper SDKs compile; provider endpoints stay explicitly delegated."""
import json
import shlex
import sys
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[2]
root = Path(tempfile.mkdtemp(prefix='forklaunch-betterauth-contract-'))
app = root/'app'
cli = repo/'cli/target/debug/forklaunch'
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'CARGO_INCREMENTAL': '0', 'CARGO_TARGET_DIR': str(repo/'cli/target'), 'RUSTUP_TOOLCHAIN': 'stable'}
def run(args, cwd=root):
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True)
    assert result.returncode == 0, result.stdout[-5000:] + result.stderr[-8000:]
try:
    run([str(cli), 'init', 'application', 'auth-proof', '--path', str(app), '--language', 'typescript',
         '--modules-path', 'modules', '--database', 'postgresql', '--validator', 'zod', '--formatter',
         'prettier', '--linter', 'eslint', '--http-framework', 'express', '--runtime', 'node',
         '--test-framework', 'vitest', '-m', 'iam-better-auth', '-D', 'Auth admission proof', '-A', 'ForkLaunch', '-L', 'MIT'])
    for relative in ['server.ts', 'bootstrapper.ts', 'auth.ts']:
        file = app/'modules/iam'/relative
        if file.is_file():
            with file.open('a') as stream:
                stream.write('\nthrow new Error("AUTH_RUNTIME_MUST_NOT_BE_IMPORTED");\n')
    run(['pnpm', 'install', '--ignore-scripts'], app/'modules')
    run([sys.executable, str(repo/'cli/tests/fixtures/worker_runtime_lock.py'), '-c', shlex.join(['pnpm', '--dir', str(app/'modules'), 'build'])], repo)
    artifact = app/'.forklaunch/contracts/iam/contract.json'
    contract = json.loads(artifact.read_text())
    assert len(contract['routes']) == 7
    surface, = contract['delegatedSurfaces']
    assert surface == {
        'id':'better-auth', 'provider':'better-auth', 'coverage':'provider-owned',
        'client':{'package':'better-auth/client','export':'createAuthClient'},
        'runtime':{'basePath':'runtime-resolved','openApi':True},
        'callbacks':['/test-callback','/oauth-redirect']
    }, surface
    config = json.loads((app / '.forklaunch/build.json').read_text())['contracts']['iam']
    assert all((app/config[key]).is_file() for key in ['typescript','rust','openapi'])
    assert 'signIn' not in (app/config['typescript']).read_text()
    print('PASS normal Better Auth build emits wrapper SDKs and explicit provider ownership without runtime imports', flush=True)
    analysis = subprocess.run([str(cli), 'analyze', '--path', str(app), '--pretty'], env=env, capture_output=True, text=True)
    assert 'FLR-CONTRACT-001' in analysis.stdout and 'provider-owned' in analysis.stdout, analysis.stdout + analysis.stderr
    before = artifact.read_bytes()
    server = app/'modules/iam/server.ts'
    original = server.read_text()
    # The explicit provider boundary must not admit unrelated routes inside it.
    mutations = [
        original.replace('mount.paths,', "'/undeclared',", 1),
        original.replace('/test-callback`', '/undeclared`', 1),
        original + "\napp.internal.get('/undeclared', () => {});\n"
    ]
    for mutation in mutations:
        assert mutation != original
        server.write_text(mutation)
        result = subprocess.run([str(cli), 'sdk', 'build', '--skip-compile', '--service', 'iam', '--path', str(app)], env=env, capture_output=True, text=True)
        assert result.returncode != 0, result.stdout
        assert 'AUTH_RUNTIME_MUST_NOT_BE_IMPORTED' not in result.stderr
        assert artifact.read_bytes() == before
    server.write_text(original)
    descriptor = app/'modules/iam/api/contracts/auth.surface.ts'
    source = descriptor.read_text()
    descriptor.write_text(source.replace("id: 'better-auth',", "id: 'better-auth', secret: 'must-never-export',"))
    rejected = subprocess.run([str(cli), 'sdk', 'build', '--skip-compile', '--service', 'iam', '--path', str(app)], env=env, capture_output=True, text=True)
    assert rejected.returncode != 0 and artifact.read_bytes() == before
    descriptor.write_text(source)
    print('PASS analyzer exposes exclusions and unrelated raw/delegated routes fail before artifact replacement', flush=True)
    run(['pnpm', 'install', '--ignore-scripts'], app)
    run(['pnpm', 'build'], app/'sdks/typescript')
    run([sys.executable, str(repo/'cli/tests/fixtures/worker_runtime_lock.py'), '-c', shlex.join([
        'cargo', '+stable', 'check', '--manifest-path', str(app/'sdks/rust/iam-sdk/Cargo.toml')])], repo)
    print('PASS generated Better Auth wrapper TypeScript and Rust SDK consumers compile', flush=True)


finally:
    if os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        print('Fixture retained:', root, flush=True)
    else:
        shutil.rmtree(root)
