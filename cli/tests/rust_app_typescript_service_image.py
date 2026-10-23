#!/usr/bin/env python3
"""A TypeScript service added to a Rust application installs and builds its image
from the nested modules workspace: native SDK packages it depends on are workspace
links (never registry lookups), and Dockerfile.typescript installs frozen.

Set FORKLAUNCH_TEST_DOCKER=1 to also build the TypeScript image."""
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
cli = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch'))
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'RUSTUP_TOOLCHAIN': os.environ.get('RUSTUP_TOOLCHAIN', 'stable')}
root = Path(tempfile.mkdtemp(prefix='forklaunch-rust-ts-image-'))
app = root / 'app'


def run(args, cwd=root, timeout=1800):
    print('Running:', ' '.join(map(str, args)), flush=True)
    result = subprocess.run(args, cwd=cwd, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=timeout)
    assert result.returncode == 0, result.stdout[-6000:] + result.stderr[-6000:]
    return result


def forklaunch(*args):
    return run([str(cli), *args, '--path', str(app)])


try:
    forklaunch('init', 'application', 'image-proof', '--language', 'rust', '--modules-path', 'src', '-D', 'Proof', '-A', 'ForkLaunch', '-L', 'MIT')
    forklaunch('init', 'module', '--language', 'rust', '-m', 'iam-base', '-d', 'postgresql')
    forklaunch('init', 'service', 'intake', '--language', 'typescript', '-d', 'postgresql', '-D', 'Intake')
    nested = (app / 'src/pnpm-workspace.yaml').read_text()
    for pattern in ["'*/sdk/typescript'", 'client-sdk/typescript']:
        assert pattern in nested, f'{pattern} missing from the nested workspace:\n{nested}'
    run(['pnpm', 'install', '--lockfile-only'], cwd=app / 'src', timeout=900)
    lock = (app / 'src/pnpm-lock.yaml').read_text()
    assert 'link:../iam/sdk/typescript' in lock, 'the iam SDK must resolve from the workspace'
    docker = (app / 'Dockerfile.typescript').read_text()
    assert 'pnpm install --frozen-lockfile' in docker
    assert 'COPY ["src/pnpm-workspace.yaml"' in docker and 'COPY ["src/iam/sdk/typescript/"' in docker
    print('PASS nested workspace links native SDKs; lockfile resolves without registry lookups', flush=True)
    if os.environ.get('FORKLAUNCH_TEST_DOCKER') == '1':
        run(['cargo', 'generate-lockfile'], cwd=app, timeout=900)
        run(['docker', 'compose', 'build', 'intake'], cwd=app, timeout=3600)
        print('PASS Dockerfile.typescript builds with a frozen install', flush=True)
finally:
    if os.environ.get('FORKLAUNCH_KEEP_FIXTURE') != '1':
        shutil.rmtree(root, ignore_errors=True)
