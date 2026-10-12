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
# Exercise automatic socket export and owned SDK regeneration in the Bun run.
# This checks artifact lifecycle; the separate socket fixtures exercise wire transport.
socket_source = app / 'crates/native-api/src/lib.rs'
socket_registration = '    registry.register_socket("development", &serde_json::json!({"path":"/dev-socket-initial","clientMessages":{"request":{"shape":{"type":"string"}}},"serverMessages":{"reply":{"shape":{"type":"string"}}}})).unwrap();\n'
if runtime == 'bun':
    cargo = app / 'crates/native-api/Cargo.toml'
    cargo.write_text(cargo.read_text().replace('[dependencies]\n', '[dependencies]\nserde_json = \"1\"\n', 1))
    socket_source.write_text(socket_source.read_text().replace('    registry\n}', socket_registration + '    registry\n}', 1))
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
args = [str(binary), 'dev', 'native-api', 'legacy-api', '--path', str(app), '--project-name', name, '--compose-file', str(fixture), '--no-deps', '--export-contracts']
if runtime == 'bun':
    args += ['--sdk-target', 'typescript', '--sdk-source', 'all']
original = {str(p.relative_to(app)): p.read_bytes() for p in app.rglob('*') if p.is_file()}
preview = subprocess.run(args + ['--dryrun'], cwd=app, env=env, capture_output=True, text=True, check=True, timeout=60)
plan = json.loads(preview.stdout)
assert plan['watch'] and set(plan['nativeServices']) == {'native-api'}
assert set(plan['typescriptServices']) == {'legacy-api'}
assert original == {str(p.relative_to(app)): p.read_bytes() for p in app.rglob('*') if p.is_file()}, 'Dev dry-run changed application files'
log_path = root / 'dev.log'
log = log_path.open('w')
process = None
sdk_watch = None
sdk_log = (root / 'sdk-watch.log').open('w')
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
    if runtime == 'node':
        sdk_watch = subprocess.Popen([str(binary), 'sdk', 'watch', '--path', str(app), '--target', 'typescript', '--interval-ms', '100'], cwd=app, env=env, stdout=sdk_log, stderr=subprocess.STDOUT)
        time.sleep(1)
        assert sdk_watch.poll() is None, 'SDK watch exited before first contract export'
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
    def wait_sdk(expected, timeout=60):
        deadline = time.monotonic() + timeout
        path = app / 'sdks/typescript/src/index.ts'
        while time.monotonic() < deadline:
            assert sdk_watch is None or sdk_watch.poll() is None, 'SDK watch exited'
            if path.exists() and expected in path.read_text():
                return
            time.sleep(0.2)
        raise AssertionError(f'SDK did not refresh: {path}')
    def wait_socket(expected, timeout=180):
        path = app / 'sdks/typescript-websocket/native-api/src/development.ts'
        artifact = app / '.forklaunch/socket-contracts/native-api/socket-contract.json'
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            assert process.poll() is None, log_path.read_text()[-20000:]
            if expected is None:
                if not path.exists() and not artifact.exists():
                    return
            elif path.exists() and expected in path.read_text() and artifact.exists() and expected in artifact.read_text():
                return
            time.sleep(0.2)
        raise AssertionError(f'Socket export/SDK did not refresh to {expected}: {log_path.read_text()[-12000:]}')
    if runtime == 'bun':
        wait_socket('/dev-socket-initial')
    wait_sdk("recordCount")
    wait_sdk("legacyApi")
    assert (app / ".forklaunch/openapi/legacy-api/openapi.json").is_file()
    contract = app / '.forklaunch/contracts/native-api/contract.json'
    assert json.loads(contract.read_text())['generator'] == 'framework-rs'
    source = app / 'crates/native-api/src/routes/health.rs'
    source.write_text(source.read_text().replace('"ok".to_string()', '"native-reloaded".to_string()', 1).replace('"const":"ok"', '"const":"native-reloaded"', 1).replace('"properties":{"service"', '"properties":{"developmentMarker":{"type":"boolean"},"service"', 1))
    native = wait_json(lambda: endpoint('native-api', native_port) + '/health', 'status', 'native-reloaded', timeout=180).removesuffix('/health')
    wait_sdk("developmentMarker")
    assert 'native-reloaded' in contract.read_text()
    if runtime == 'bun':
        original_socket_source = socket_source.read_text()
        socket_source.write_text(original_socket_source.replace('/dev-socket-initial', '/dev-socket-reloaded'))
        wait_socket('/dev-socket-reloaded')
        socket_source.write_text(original_socket_source.replace(socket_registration, ''))
        wait_socket(None)
        socket_source.write_text(original_socket_source.replace('/dev-socket-initial', '/dev-socket-restored'))
        wait_socket('/dev-socket-restored')
        wait_sdk('developmentMarker')
        native = wait_json(lambda: endpoint('native-api', native_port) + '/health', 'status', 'native-reloaded', timeout=180).removesuffix('/health')
    source = app / 'modules/legacy-api/api/controllers/legacyApi.controller.ts'
    source.write_text(source.read_text().replace("message: 'LegacyApi'", "message: 'LegacyApiReloaded'", 1))
    schema = app / 'modules/legacy-api/domain/schemas/legacyApi.schema.ts'
    schema.write_text(schema.read_text().replace('import { string }', 'import { string, optional }').replace('export const LegacyApiResponseSchema = {', 'export const LegacyApiResponseSchema = { tsDevelopmentMarker: optional(string),'))
    wait_sdk('tsDevelopmentMarker', timeout=120)
    wait_json(lambda: endpoint('legacy-api', ts_port) + '/legacy-api', 'message', 'LegacyApiReloaded', timeout=120)
    core = app / 'modules/core'
    shared = core / 'development.schema.ts'
    shared.write_text("import { optional, string } from './registrations';\nexport const sharedDevelopmentSchema = optional(string);\n")
    index = core / 'index.ts'
    index.write_text(index.read_text() + "\nexport * from './development.schema';\n")
    core_name = json.loads((core / 'package.json').read_text())['name']
    schema.write_text(f"import {{ sharedDevelopmentSchema }} from '{core_name}';\n" + schema.read_text().replace('export const LegacyApiResponseSchema = {', 'export const LegacyApiResponseSchema = { sharedLibrary: sharedDevelopmentSchema,'))
    wait_sdk('sharedLibrary', timeout=120)
    wait_json(lambda: endpoint('legacy-api', ts_port) + '/legacy-api', 'message', 'LegacyApiReloaded', timeout=120)
    shared.write_text("import { optional, string } from './registrations';\nexport const sharedDevelopmentSchema = optional({ sharedLibraryMarker: optional(string) });\n")
    wait_sdk('sharedLibraryMarker', timeout=120)
    typescript = wait_json(lambda: endpoint('legacy-api', ts_port) + '/legacy-api', 'message', 'LegacyApiReloaded', timeout=60).removesuffix('/legacy-api')
    mcp = endpoint('legacy-api', ts_port + 2000) + '/mcp'
    subprocess.run(['node', str(repo / 'cli/tests/rust_mixed_dev_probe.mjs'), native, typescript, mcp, 'LegacyApiReloaded'], check=True, timeout=90)
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=90) == 0
    assert all(not container['State']['Running'] for container in inspect(containers))
    if runtime == 'bun':
        commands = subprocess.check_output(['ps', '-axo', 'command'], text=True)
        assert f'sdk watch --path {app} --target typescript' not in commands, 'Owned SDK watcher survived development shutdown'

    for file in ['docker-compose.yaml', 'Dockerfile', 'Dockerfile.rust', 'modules/Dockerfile']:
        if file in original:
            assert (app / file).read_bytes() == original[file]
    print(f'PASS {runtime} mixed dev startup, Rust/TypeScript source reloads, native/TypeScript/shared-library contract export and combined SDK refresh, HTTP/MCP before/after edits, SIGTERM supervision and preserved production configuration: {root}', flush=True)
finally:
    if process is not None and process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=60)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    if sdk_watch is not None and sdk_watch.poll() is None:
        sdk_watch.terminate()
        sdk_watch.wait(timeout=10)
    sdk_log.close()
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

# Reuse generated sources to validate TypeScript-only selection without compiling
# a second native workspace. The replay owns a distinct Compose project.
subprocess.run([os.environ.get('PYTHON', 'python3'), str(repo / 'cli/tests/rust_mixed_dev_typescript_replay.py'), str(app)], check=True, env=env)
