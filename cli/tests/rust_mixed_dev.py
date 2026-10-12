#!/usr/bin/env python3
"""Actual mixed Compose development startup, source reloads, MCP and CLI shutdown."""
import base64
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import urllib.request
import uuid

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-mixed-dev'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
app = root / 'app'
name = 'dev-fixture-' + uuid.uuid4().hex[:10].translate(str.maketrans('0123456789', 'ghijklmnop'))
runtime = os.environ.get('FORKLAUNCH_TEST_DEV_RUNTIME', 'node')
assert runtime in ['node', 'bun']
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'FORKLAUNCH_ACCOUNT': 'parity-offline-dev', 'CARGO_INCREMENTAL': '0'}
def cli(*args):
    return subprocess.run([str(binary), *args, '--path', str(app)], cwd=root, env=env, check=True, timeout=180)
if runtime == 'node':
    cli('init', 'application', name, '--language', 'rust', '-D', 'Mixed development', '-A', 'ForkLaunch', '-L', 'MIT')
else:
    cli('init', 'application', name, '--language', 'typescript', '--modules-path', 'modules',
        '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'eslint',
        '--http-framework', 'express', '--runtime', 'bun', '--test-framework', 'vitest',
        '-D', 'Mixed development', '-A', 'ForkLaunch', '-L', 'MIT')
cli('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
cli('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API')
base = ['docker', 'compose', '--project-directory', str(app), '-p', name, '-f', str(app / 'docker-compose.yaml')]
configuration = json.loads(subprocess.check_output(base + ['config', '--format', 'json'], env=env))
native_port = int(configuration['services']['native-api']['environment']['PORT'])
ts_port = int(configuration['services']['legacy-api']['environment']['PORT'])
fixture = app / 'dev.fixture.yaml'
fixture.write_text(f'''services:
  native-api:
    ports: !override
      - "127.0.0.1:0:{native_port}"
    environment:
      CARGO_BUILD_JOBS: "2"
      OTEL_LEVEL: error
    healthcheck:
      disable: true
  legacy-api:
    ports: !override
      - "127.0.0.1:0:{ts_port}"
      - "127.0.0.1:0:{ts_port + 2000}"
    environment:
      DOTENV_FILE_PATH: .env.local
      HMAC_SECRET_KEY: fixture-only-hmac-secret
      ENCRYPTION_KEY: {base64.b64encode(bytes([19] * 32)).decode()}
      OTEL_LEVEL: error
      OTEL_EXPORTER_OTLP_ENDPOINT: http://127.0.0.1:9
    healthcheck:
      disable: true
''')
args = [str(binary), 'dev', 'native-api', 'legacy-api', '--path', str(app), '--project-name', name, '--compose-file', str(fixture), '--no-deps']
original = {str(p.relative_to(app)): p.read_bytes() for p in app.rglob('*') if p.is_file()}
preview = subprocess.run(args + ['--dryrun'], cwd=app, env=env, capture_output=True, text=True, check=True, timeout=60)
plan = json.loads(preview.stdout)
assert plan['watch'] and set(plan['nativeServices']) == {'native-api'}
assert original == {str(p.relative_to(app)): p.read_bytes() for p in app.rglob('*') if p.is_file()}, 'Dev dry-run changed application files'
log_path = root / 'dev.log'
log = log_path.open('w')
process = None
containers = []
def owned_ids():
    return subprocess.check_output(['docker', 'ps', '-a', '--filter', f'label=com.docker.compose.project={name}', '--format', '{{.ID}}'], text=True).split()
def inspect(ids):
    return json.loads(subprocess.check_output(['docker', 'inspect', *ids])) if ids else []
def endpoint(service, port):
    for container in inspect(owned_ids()):
        if container['Config']['Labels']['com.docker.compose.service'] == service:
            bindings = container['NetworkSettings']['Ports'].get(f'{port}/tcp')
            if bindings:
                return 'http://127.0.0.1:' + bindings[0]['HostPort']
    raise KeyError(f'No active port for {service}')
def wait_json(url, field, expected, timeout=900):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        assert process.poll() is None, log_path.read_text()[-20000:]
        try:
            address = url() if callable(url) else url
            with urllib.request.urlopen(address, timeout=2) as response:
                if json.load(response).get(field) == expected:
                    return address
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(1)
    raise AssertionError(f'Timed out waiting for {url}: {log_path.read_text()[-20000:]}')
try:
    process = subprocess.Popen(args, cwd=app, env=env, stdout=log, stderr=subprocess.STDOUT)
    deadline = time.monotonic() + 1200
    endpoints = {}
    while time.monotonic() < deadline:
        assert process.poll() is None, log_path.read_text()[-20000:]
        containers = owned_ids()
        for container in inspect(containers):
            service = container['Config']['Labels']['com.docker.compose.service']
            ports = container['NetworkSettings']['Ports'] or {}
            for port, bindings in ports.items():
                if bindings:
                    endpoints[(service, int(port.split('/')[0]))] = 'http://127.0.0.1:' + bindings[0]['HostPort']
        if all(key in endpoints for key in [('native-api', native_port), ('legacy-api', ts_port), ('legacy-api', ts_port + 2000)]):
            break
        time.sleep(2)
    native = endpoints[('native-api', native_port)]
    typescript = endpoints[('legacy-api', ts_port)]
    mcp = endpoints[('legacy-api', ts_port + 2000)] + '/mcp'
    wait_json(native + '/health', 'status', 'ok')
    wait_json(typescript + '/legacy-api', 'message', 'LegacyApi')
    subprocess.run(['node', str(repo / 'cli/tests/rust_mixed_dev_probe.mjs'), native, typescript, mcp, 'LegacyApi'], check=True, timeout=90)
    source = app / 'crates/native-api/src/routes/health.rs'
    source.write_text(source.read_text().replace('"ok".to_string()', '"native-reloaded".to_string()', 1))
    native = wait_json(lambda: endpoint('native-api', native_port) + '/health', 'status', 'native-reloaded', timeout=180).removesuffix('/health')
    source = app / 'modules/legacy-api/api/controllers/legacyApi.controller.ts'
    source.write_text(source.read_text().replace("message: 'LegacyApi'", "message: 'LegacyApiReloaded'", 1))
    typescript = wait_json(lambda: endpoint('legacy-api', ts_port) + '/legacy-api', 'message', 'LegacyApiReloaded', timeout=60).removesuffix('/legacy-api')
    mcp = endpoint('legacy-api', ts_port + 2000) + '/mcp'
    subprocess.run(['node', str(repo / 'cli/tests/rust_mixed_dev_probe.mjs'), native, typescript, mcp, 'LegacyApiReloaded'], check=True, timeout=90)
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=90) == 0
    assert all(not container['State']['Running'] for container in inspect(containers))
    for file in ['docker-compose.yaml', 'Dockerfile', 'Dockerfile.rust', 'modules/Dockerfile']:
        if file in original:
            assert (app / file).read_bytes() == original[file]
    print(f'PASS {runtime} mixed dev startup, Rust/TypeScript source reloads, HTTP/MCP before/after edits, SIGTERM supervision and preserved production configuration: {root}', flush=True)
finally:
    if process is not None and process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=60)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    log.close()
    ids = owned_ids()
    if ids:
        subprocess.run(['docker', 'rm', '-f', '-v', *ids], check=True)
    for resource in ['volume', 'network']:
        ids = subprocess.check_output(['docker', resource, 'ls', '--filter', f'label=com.docker.compose.project={name}', '--format', '{{.ID}}' if resource == 'network' else '{{.Name}}'], text=True).split()
        if ids:
            subprocess.run(['docker', resource, 'rm', *ids], check=True)
    for image in [configuration['services']['legacy-api']['image'], f'forklaunch-dev-{name}-native-api:local']:
        subprocess.run(['docker', 'image', 'rm', image], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
