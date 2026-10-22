#!/usr/bin/env python3
"""Compile generated documentation with current snapshots and published 0.1.0."""
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from urllib.request import urlopen

repo = Path(__file__).resolve().parents[2]
env = {**os.environ, 'CARGO_INCREMENTAL': '0', 'RUSTUP_TOOLCHAIN': 'stable',
       'FORKLAUNCH_SKIP_FORMAT': '1'}
env.setdefault('CARGO_TARGET_DIR', str(repo / 'cli/target'))
if not env.get('FORKLAUNCH_WORKER_RUNTIME_LOCKED'):
    sys.exit(subprocess.run([sys.executable, str(repo / 'cli/tests/fixtures/worker_runtime_lock.py'),
        '-c', shlex.join([sys.executable, str(Path(__file__).resolve())])], env=env).returncode)
cli = Path(env.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
root = Path(tempfile.mkdtemp(prefix='forklaunch-native-docs-')).resolve()
print('CLI SHA256:', hashlib.sha256(cli.read_bytes()).hexdigest(), flush=True)


def run(*args, cwd):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd, env=env, check=True, timeout=1800)


try:
    for mode, selection in [('snapshot', []), ('registry-0.1.0', ['--framework-version', '0.1.0'])]:
        app = root / mode
        run(cli, 'init', 'application', 'docs-proof', '--path', app, '--database', 'postgresql',
            '--validator', 'zod', '--formatter', 'prettier', '--linter', 'oxlint',
            '--http-framework', 'express', '--runtime', 'node', '--test-framework', 'vitest',
            '-D', 'Native documentation', '-A', 'ForkLaunch', '-L', 'MIT', *selection, cwd=root)
        run(cli, 'init', 'service', 'docs-api', '--language', 'rust', '--path', app, cwd=root)
        helper = 'with_docs' if mode == 'snapshot' else 'with_openapi'
        source = (app / 'src/docs-api/application.rs').read_text()
        # The current template imports `forklaunch_axum::native`; the 0.1.0 one
        # spells the path out. Either way exactly the selected helper is called.
        other = 'with_openapi' if helper == 'with_docs' else 'with_docs'
        assert f'native::{helper}(' in source and f'native::{other}(' not in source
        if mode != 'snapshot':
            assert not (app / 'vendor/framework').exists()
            assert '=0.1.0' in (app / 'src/docs-api/Cargo.toml').read_text()
        application = app / 'src/docs-api/application.rs'
        authored = source + '\npub const APPLICATION_OWNED: &str = "preserved";\n'
        application.write_text(authored)
        run(cli, 'init', 'service', 'docs-api', '--language', 'rust', '--path', app, cwd=root)
        assert application.read_text() == authored, 'Reinitialization replaced application composition'
        if mode != 'snapshot':
            # Generated configuration reads forklaunch_core::config, added in
            # framework 0.2.0; the 0.1.0 selection is checked at the source level.
            print('PASS', mode, 'generated helper selection and source preservation', flush=True)
            continue
        run('cargo', 'build', '-p', 'docs-api', cwd=app)
        runtime_env = {key: value for key, value in env.items()
                       if not key.startswith(('MCP_', 'OTEL_'))}
        runtime_env.update(HOST='127.0.0.1', PORT='0', PROTOCOL='http', VERSION='v1', DOCS_PATH='/docs',
                           RUST_LOG='info', MCP_ENABLED='false')
        child = subprocess.Popen([str(Path(env['CARGO_TARGET_DIR']) / 'debug/docs-api')],
            cwd=app, env=runtime_env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        try:
            lines = []
            deadline = time.monotonic() + 20
            base = None
            while time.monotonic() < deadline:
                if select.select([child.stdout], [], [], .1)[0]:
                    line = child.stdout.readline()
                    lines.append(line)
                    matched = re.search(r'listening on (http://127\.0\.0\.1:\d+)', line)
                    if matched:
                        base = matched[1]
                        break
                assert child.poll() is None, ''.join(lines)
            assert base, ''.join(lines)
            with urlopen(base + '/health', timeout=10) as response:
                assert response.status == 200 and json.load(response)['status'] == 'ok'
            with urlopen(base + '/openapi.json', timeout=10) as response:
                assert response.headers.get_content_type() == 'application/json'
                document = json.load(response)
            assert '/health' in document['paths']
            with urlopen(base + '/docs', timeout=10) as response:
                content_type = response.headers.get_content_type()
                content = response.read().decode()
            if mode == 'snapshot':
                assert content_type == 'text/html'
                assert 'scalar' in content.lower() and '/openapi.json' in content
                operation = document['paths']['/health']['get']
                assert 'x-forklaunch-operation' in operation
            else:
                assert content_type == 'application/json' and json.loads(content) == document
            print('PASS', mode, 'generated helper, real Cargo build, HTTP health, canonical OpenAPI and /docs content', flush=True)
        finally:
            child.terminate()
            try:
                code = child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
                raise AssertionError('Documentation service failed graceful shutdown')
            assert code == 0, code
finally:
    if env.get('FORKLAUNCH_KEEP_FIXTURE'):
        print('Native docs fixture evidence:', root, flush=True)
    else:
        shutil.rmtree(root)
