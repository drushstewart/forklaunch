#!/usr/bin/env python3
"""Real socket/AsyncAPI compilation in an app using published framework crates."""
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile

repo = Path(__file__).resolve().parents[2]
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1',
       'CARGO_INCREMENTAL': '0', 'RUSTUP_TOOLCHAIN': 'stable'}
env.setdefault('CARGO_TARGET_DIR', str(repo / 'cli/target'))
if not env.get('FORKLAUNCH_WORKER_RUNTIME_LOCKED'):
    sys.exit(subprocess.run([sys.executable, str(repo / 'cli/tests/fixtures/worker_runtime_lock.py'),
        '-c', shlex.join([sys.executable, str(Path(__file__).resolve())])], env=env).returncode)
cli = Path(env.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
root = Path(tempfile.mkdtemp(prefix='forklaunch-socket-compiler-')).resolve()
app = root / 'app'
print('CLI SHA256:', hashlib.sha256(cli.read_bytes()).hexdigest(), flush=True)


def run(*arguments, overrides=None, success=True):
    command = [str(cli), *arguments, '--path', str(app)]
    print('Running:', shlex.join(command), 'with', overrides or 'default compiler', flush=True)
    result = subprocess.run(command, cwd=root, env={**env, **(overrides or {})},
                            capture_output=True, text=True, timeout=600)
    print(result.stdout, end='', flush=True)
    print(result.stderr, end='', flush=True)
    assert (result.returncode == 0) == success, result.returncode
    return result


def snapshot():
    return {str(path.relative_to(app)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in app.rglob('*') if path.is_file()}


try:
    run('init', 'application', 'socket-compiler', '--database', 'postgresql', '--validator', 'zod',
        '--formatter', 'prettier', '--linter', 'eslint', '--http-framework', 'express',
        '--runtime', 'node', '--test-framework', 'vitest', '-D', 'Published socket compiler',
        '-A', 'ForkLaunch', '-L', 'MIT', '--framework-version', '0.1.0')
    run('init', 'service', 'chat', '--language', 'rust')
    for source, fixture, relative in [
        ('socket-contract', 'rust_socket_contract.json', '.forklaunch/socket-contracts/chat/socket-contract.json'),
        ('asyncapi', 'rust_asyncapi_socket.json', '.forklaunch/asyncapi/chat/asyncapi.json'),
    ]:
        artifact = app / relative
        artifact.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(repo / 'cli/tests/fixtures' / fixture, artifact)
        for overrides in [{}, {'FORKLAUNCH_SDK_ZOD_CHECKS': '1'}]:
            run('sdk', 'generate', 'all', '--from', source, '--service', 'chat', overrides=overrides)
            assert (app / 'src/client-sdk/typescript-websocket/chat/index.ts').is_file()
            cargo = (app / 'src/client-sdk/rust/chat-websocket-sdk/Cargo.toml').read_text()
            assert '=0.1.0' in cargo and 'vendor/framework' not in cargo
            assert not (app / 'vendor/framework').exists()
            assert not (app / '.forklaunch/framework-rs').exists()
            before = snapshot()
            run('sdk', 'generate', 'all', '--from', source, '--service', 'chat', '--check', overrides=overrides)
            assert snapshot() == before, 'Freshness check changed application files'
            print('PASS source', source, 'options', json.dumps(overrides), flush=True)
        before = snapshot()
        rejected = run('sdk', 'generate', 'all', '--from', source, '--service', 'chat',
                       overrides={'FORKLAUNCH_SDK_ZOD_CHECKS': 'true'}, success=False)
        assert 'must be unset, 0 or 1' in rejected.stdout + rejected.stderr
        assert snapshot() == before, 'Invalid compiler option changed application files'
    print('PASS both socket sources with published0.1.0 dependencies, default and opted-in compiler, and read-only checks', flush=True)
finally:
    if env.get('FORKLAUNCH_KEEP_FIXTURE'):
        print('Socket compiler fixture evidence:', root, flush=True)
    else:
        shutil.rmtree(root)
