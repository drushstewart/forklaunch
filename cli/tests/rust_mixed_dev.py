#!/usr/bin/env python3
"""Actual mixed Compose development startup, source reloads, MCP and CLI shutdown."""
import base64
import atexit
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
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
# Keep standalone watcher coverage available while the default matrix proves
# dev-owned initial SDK generation and dependency-lock preparation in both runtimes.
external_sdk_watch = runtime == 'node' and os.environ.get('FORKLAUNCH_TEST_DEV_EXTERNAL_SDK_WATCH') == '1'
def cleanup_fixture():
    if not root.exists() or os.environ.get('FORKLAUNCH_KEEP_FIXTURE') == '1':
        return
    destination = Path(os.environ.get('FORKLAUNCH_DEV_LOG_DIR', tempfile.gettempdir()))
    destination.mkdir(parents=True, exist_ok=True)
    for path in root.glob('*.log'):
        shutil.copy2(path, destination / f'framework-mixed-dev-{runtime}-{path.name}')
    shutil.rmtree(root)
atexit.register(cleanup_fixture)
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'FORKLAUNCH_ACCOUNT': 'parity-offline-dev', 'CARGO_INCREMENTAL': '0', 'HUSKY': '0'}
print(f'RUN {runtime} mixed dev: CLI {binary} SHA-256 {hashlib.sha256(binary.read_bytes()).hexdigest()}', flush=True)
def cli(*args):
    return subprocess.run([str(binary), *args, '--path', str(app)], cwd=root, env=env, check=True, timeout=180)
cli('init', 'application', name,
    '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'oxlint',
    '--http-framework', 'express', '--runtime', runtime, '--test-framework', 'vitest',
    '-D', 'Mixed development', '-A', 'ForkLaunch', '-L', 'MIT')
cli('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
cli('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API')
# Exercise automatic socket export and owned SDK regeneration in the Bun run.
# This checks artifact lifecycle; the separate socket fixtures exercise wire transport.
socket_source = app / 'src/native-api/.forklaunch/contracts/lib.rs'
socket_registration = '    development_sockets::register_into(&mut registry).unwrap();\n'
socket_binding = '    declarations.extend(development_sockets::binding_manifest()?);\n'
if runtime == 'bun':
    # This fixture extends the private compiler host only to exercise registry
    # socket export ownership; socket wire/security tests own protocol coverage.
    # The compiler checks a complete static declaration manifest. Keep the
    # native runtime registry limited to its actually bound HTTP endpoints.
    original_host = socket_source.read_text()
    assert original_host.count('pub fn registry()') == 1
    assert original_host.count('pub fn binding_manifest()') == 1
    socket_source.write_text(original_host.replace('pub fn registry()', 'pub fn native_registry()')
        .replace('pub fn binding_manifest()', 'pub fn native_binding_manifest()') + '''
#[forklaunch_core::ws::socket_api]
pub mod development_sockets {
    pub fn development() -> forklaunch_core::ws::TypedSocket<
        forklaunch_core::contract::endpoint::NoBody, String, String
    > {
        forklaunch_core::ws::TypedSocket::new(
            forklaunch_core::contract::RouteContract::get(
                "Development", "Development", "/dev-socket-initial"
            ), "request", "reply",
        ).unwrap()
    }
}
pub fn registry() -> forklaunch_core::contract::ContractRegistry {
    let mut registry = native_registry();
''' + socket_registration + '''    registry
}
pub fn binding_manifest() -> Result<Vec<forklaunch_core::contract::assembly::BindingDeclaration>, String> {
    let mut declarations = native_binding_manifest()?;
''' + socket_binding + '''    Ok(declarations)
}
''')
    for relative in ['api/routes/mod.rs', 'application.rs']:
        path = app / 'src/native-api' / relative
        original_runtime = path.read_text()
        assert 'contracts::registry()' in original_runtime, relative
        path.write_text(original_runtime.replace('contracts::registry()', 'contracts::native_registry()'))
# The generated workspace README installs and builds before development; the
# shared production image requires those real package-manager and Cargo locks.
# Build native declarations first so the TypeScript workspace can consume them.
manager = 'bun' if runtime == 'bun' else 'pnpm'
for command, directory in [
    (['cargo', 'build', '--workspace'], app),
    ([manager, 'install'], app / 'src'),
    ([manager, 'run', 'build'], app / 'src'),
    (['cargo', 'generate-lockfile'], app),
]:
    print('RUN documented bootstrap:', ' '.join(command), 'in', directory, flush=True)
    subprocess.run(command, cwd=directory, env=env, check=True, timeout=1800)
assert (app / 'Cargo.lock').is_file()
assert (app / 'src' / ('bun.lock' if runtime == 'bun' else 'pnpm-lock.yaml')).is_file()
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
if not external_sdk_watch:
    args += ['--sdk-target', 'all', '--sdk-source', 'all' if runtime == 'bun' else 'http']
def application_files():
    files = {}
    for directory, children, names in os.walk(app):
        children[:] = [name for name in children if name not in {'node_modules', 'target', '.git'}]
        for name in names:
            path = Path(directory) / name
            if path.is_file():
                files[str(path.relative_to(app))] = path.read_bytes()
    return files
original = application_files()
preview = subprocess.run(args + ['--dryrun'], cwd=app, env=env, capture_output=True, text=True, check=True, timeout=60)
plan = json.loads(preview.stdout)
assert plan['watch'] and set(plan['nativeServices']) == {'native-api'}
assert set(plan['typescriptServices']) == {'legacy-api'}
assert original == application_files(), 'Dev dry-run changed application sources, lockfiles or generated artifacts'
log_path = root / 'dev.log'
log = log_path.open('w')
process = None
replay_ready = False
sdk_watch = None
sdk_log = (root / 'sdk-watch.log').open('w')
containers = []
typescript_images = set()
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
def probe_surfaces(expected, native_status='ok', timeout=180):
    # Initial Cargo exports and SDK generation can cause a Compose restart after
    # the first successful health response. Require stable container epochs and
    # resolve ephemeral ports again; protocol assertion failures still fail.
    deadline = time.monotonic() + timeout
    observed = None
    stable_since = time.monotonic()
    last_failure = ''
    while time.monotonic() < deadline:
        assert process.poll() is None, log_path.read_text()[-20000:]
        active = inspect(owned_ids())
        epochs = sorted((item['Id'], item['State']['StartedAt'], item['State']['Running'],
            json.dumps(item['NetworkSettings']['Ports'], sort_keys=True)) for item in active)
        if epochs != observed:
            observed, stable_since = epochs, time.monotonic()
        if len(active) != 2 or not all(item['State']['Running'] for item in active) or time.monotonic() - stable_since < 3:
            time.sleep(0.5)
            continue
        try:
            native = endpoint('native-api', native_port)
            typescript = endpoint('legacy-api', ts_port)
            mcp = endpoint('legacy-api', ts_port + 2000) + '/mcp'
            for url, field, value in [(native + '/health', 'status', native_status),
                (typescript + '/legacy-api', 'message', expected)]:
                with urllib.request.urlopen(url, timeout=2) as response:
                    assert json.load(response).get(field) == value, url
        except (OSError, ValueError, KeyError):
            time.sleep(0.5)
            continue
        result = subprocess.run(['node', str(repo / 'cli/tests/rust_mixed_dev_probe.mjs'), native, typescript, mcp, expected],
            capture_output=True, text=True, timeout=90)
        if result.returncode == 0:
            print(result.stdout, end='', flush=True)
            return
        last_failure = result.stdout + result.stderr
        if not any(code in last_failure for code in ['ECONNREFUSED', 'ECONNRESET', 'UND_ERR_SOCKET']):
            raise AssertionError(last_failure)
        # A bounded transport retry tolerates a restart in the narrow interval
        # between inspection and the probe; failed protocol responses never retry.
        stable_since = time.monotonic()
        time.sleep(0.5)
    raise AssertionError(f'HTTP/MCP endpoints did not settle: {last_failure}\n{log_path.read_text()[-20000:]}')
try:
    if external_sdk_watch:
        sdk_watch = subprocess.Popen([str(binary), 'sdk', 'watch', '--path', str(app), '--target', 'all', '--from', 'openapi', '--interval-ms', '100'], cwd=app, env=env, stdout=sdk_log, stderr=subprocess.STDOUT)
        time.sleep(1)
        assert sdk_watch.poll() is None, 'SDK watch exited before first contract export'
    print(f'PASS {runtime} documented bootstrap and dev dry-run purity; starting Docker development, live log {log_path}', flush=True)
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
    # Docker can assign a new ephemeral host port when a startup rebuild restarts.
    native = wait_json(lambda: endpoint('native-api', native_port) + '/health', 'status', 'ok').removesuffix('/health')
    typescript = wait_json(lambda: endpoint('legacy-api', ts_port) + '/legacy-api', 'message', 'LegacyApi').removesuffix('/legacy-api')
    mcp = endpoint('legacy-api', ts_port + 2000) + '/mcp'
    probe_surfaces('LegacyApi')
    def wait_sdk(expected, service='native-api', timeout=60):
        deadline = time.monotonic() + timeout
        directory = app / 'src' / service / 'sdk'
        # Rust services generate discrete TypeScript exports. TypeScript keeps
        # its authored MapToSdk exports and emits its cross-language Rust SDK.
        path = directory / ('typescript/sdk.ts' if service == 'native-api' else 'rust')
        artifact = directory / ('typescript/contract.json' if service == 'native-api' else 'openapi.json')
        tokens = {expected, re.sub(r'(?<!^)(?=[A-Z])', '_', expected).lower()}
        while time.monotonic() < deadline:
            assert sdk_watch is None or sdk_watch.poll() is None, 'SDK watch exited'
            source = path.read_text() if path.is_file() else '\n'.join(item.read_text() for item in path.glob('*.rs'))
            if any(token in source for token in tokens) and artifact.exists() and expected in artifact.read_text():
                return
            time.sleep(0.2)
        (root / f'{service}-sdk-timeout.log').write_text(json.dumps({
            'expected': expected, 'sourcePath': str(path), 'source': source,
            'artifactPath': str(artifact), 'artifact': artifact.read_text() if artifact.exists() else None,
        }, indent=2))
        raise AssertionError(f'SDK did not refresh: {path}')
    def wait_socket(expected, timeout=180):
        path = app / 'src/client-sdk/typescript-websocket/native-api/socket_development.ts'
        artifact = app / 'src/native-api/sdk/typescript/socket-contract.json'
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
    wait_sdk("legacyApi", service='legacy-api')
    assert (app / "src/legacy-api/sdk/openapi.json").is_file()
    contract = app / 'src/native-api/sdk/typescript/contract.json'
    assert json.loads(contract.read_text())['generator'] == 'framework-rs'
    source = app / 'src/native-api/domain/services/health.rs'
    schema = app / 'src/native-api/domain/schemas/mod.rs'
    assert 'status: "ok".into()' in source.read_text()
    schema.write_text(schema.read_text().replace('pub struct HealthResponse {', 'pub struct HealthResponse {\n    pub development_marker: bool,', 1))
    source.write_text(source.read_text().replace('status: "ok".into()', 'status: "native-reloaded".into(), development_marker: true', 1))
    native = wait_json(lambda: endpoint('native-api', native_port) + '/health', 'status', 'native-reloaded', timeout=180).removesuffix('/health')
    wait_sdk("developmentMarker")
    assert 'developmentMarker' in contract.read_text()
    print(f'PASS {runtime} native source reload and generated TypeScript SDK refresh', flush=True)
    if runtime == 'bun':
        original_socket_source = socket_source.read_text()
        socket_source.write_text(original_socket_source.replace('/dev-socket-initial', '/dev-socket-reloaded'))
        wait_socket('/dev-socket-reloaded')
        assert original_socket_source.count(socket_registration) == 1
        assert original_socket_source.count(socket_binding) == 1
        socket_source.write_text(original_socket_source.replace(socket_registration, '').replace(socket_binding, ''))
        wait_socket(None)
        socket_source.write_text(original_socket_source.replace('/dev-socket-initial', '/dev-socket-restored'))
        wait_socket('/dev-socket-restored')
        wait_sdk('developmentMarker')
        native = wait_json(lambda: endpoint('native-api', native_port) + '/health', 'status', 'native-reloaded', timeout=180).removesuffix('/health')
    source = app / 'src/legacy-api/api/controllers/legacyApi.controller.ts'
    source.write_text(source.read_text().replace("message: 'LegacyApi'", "message: 'LegacyApiReloaded'", 1))
    schema = app / 'src/legacy-api/domain/schemas/legacyApi.schema.ts'
    assert 'import { string }' in schema.read_text()
    assert 'export const LegacyApiResponseSchema = {' in schema.read_text()
    schema.write_text(schema.read_text().replace('import { string }', 'import { string, optional }').replace('export const LegacyApiResponseSchema = {', 'export const LegacyApiResponseSchema = { tsDevelopmentMarker: optional(string),'))
    wait_sdk('tsDevelopmentMarker', service='legacy-api', timeout=120)
    wait_json(lambda: endpoint('legacy-api', ts_port) + '/legacy-api', 'message', 'LegacyApiReloaded', timeout=120)
    print(f'PASS {runtime} TypeScript source reload and generated Rust SDK refresh', flush=True)
    core = app / 'src/core'
    shared = core / 'development.schema.ts'
    shared.write_text("import { optional, string } from './registrations';\nexport const sharedDevelopmentSchema = optional(string);\n")
    index = core / 'index.ts'
    index.write_text(index.read_text() + "\nexport * from './development.schema';\n")
    core_name = json.loads((core / 'package.json').read_text())['name']
    schema.write_text(f"import {{ sharedDevelopmentSchema }} from '{core_name}';\n" + schema.read_text().replace('export const LegacyApiResponseSchema = {', 'export const LegacyApiResponseSchema = { sharedLibrary: sharedDevelopmentSchema,'))
    wait_sdk('sharedLibrary', service='legacy-api', timeout=120)
    wait_json(lambda: endpoint('legacy-api', ts_port) + '/legacy-api', 'message', 'LegacyApiReloaded', timeout=120)
    shared.write_text("import { optional, string } from './registrations';\nexport const sharedDevelopmentSchema = optional({ sharedLibraryMarker: optional(string) });\n")
    wait_sdk('sharedLibraryMarker', service='legacy-api', timeout=120)
    typescript = wait_json(lambda: endpoint('legacy-api', ts_port) + '/legacy-api', 'message', 'LegacyApiReloaded', timeout=60).removesuffix('/legacy-api')
    native = wait_json(lambda: endpoint('native-api', native_port) + '/health', 'status', 'native-reloaded', timeout=180).removesuffix('/health')
    mcp = endpoint('legacy-api', ts_port + 2000) + '/mcp'
    probe_surfaces('LegacyApiReloaded', native_status='native-reloaded')
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=90) == 0
    assert all(not container['State']['Running'] for container in inspect(containers))
    if not external_sdk_watch:
        commands = subprocess.check_output(['ps', '-axo', 'command'], text=True)
        assert f'sdk watch --path {app} --target all' not in commands, 'Owned SDK watcher survived development shutdown'
    elif sdk_watch is not None:
        sdk_watch.terminate()
        sdk_watch.wait(timeout=10)
    # Once exporters and watchers have settled, prove that live outputs retain
    # both language types and pass the ordinary read-only freshness gate.
    subprocess.run([manager, 'run', 'sdk:typecheck'], cwd=app, env=env, check=True, timeout=600)
    for service in ['native-api', 'legacy-api']:
        subprocess.run(['cargo', 'check', '--locked', '--manifest-path',
            str(app / 'src' / service / 'sdk/rust/Cargo.toml')],
            cwd=app, env=env, check=True, timeout=600)
    cli('sdk', 'generate', '--target', 'all', '--from', 'openapi', '--check')
    if runtime == 'bun':
        cli('sdk', 'generate', '--target', 'all', '--from', 'socket-contract', '--check')
    print(f'PASS {runtime} settled SDK TypeScript/Rust type checks and read-only freshness', flush=True)

    for file in ['docker-compose.yaml', 'Dockerfile', 'Dockerfile.rust', 'Dockerfile.typescript', 'src/Dockerfile']:
        if file in original:
            assert (app / file).read_bytes() == original[file]
    replay_ready = True
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
    typescript_images.update(container['Config']['Image'] for container in inspect(ids)
        if container['Config']['Labels']['com.docker.compose.service'] == 'legacy-api')
    if ids:
        subprocess.run(['docker', 'rm', '-f', '-v', *ids], check=True)
    for resource in ['volume', 'network']:
        ids = subprocess.check_output(['docker', resource, 'ls', '--filter', f'label=com.docker.compose.project={name}', '--format', '{{.ID}}' if resource == 'network' else '{{.Name}}'], text=True).split()
        if ids:
            subprocess.run(['docker', resource, 'rm', *ids], check=True)
    images = {f'forklaunch-dev-{name}-native-api:local'}
    if not replay_ready:
        images.update(typescript_images)
        images.add(configuration['services']['legacy-api']['image'])
    for image in images:
        subprocess.run(['docker', 'image', 'rm', image], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

# Reuse generated sources to validate TypeScript-only selection without compiling
# a second native workspace. The replay owns a distinct Compose project.
try:
    subprocess.run([os.environ.get('PYTHON', 'python3'), str(repo / 'cli/tests/rust_mixed_dev_typescript_replay.py'), str(app), '--reuse-image'], check=True, env=env)
finally:
    for image in typescript_images | {configuration['services']['legacy-api']['image']}:
        subprocess.run(['docker', 'image', 'rm', image], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
