#!/usr/bin/env python3
"""Real generated builds and the explicit unsupported shared-image release guard.

Set FORKLAUNCH_EXPECT_SHARED_RELEASE_SUPPORTED=1 for the preserved positive
capability assertions once a versioned platform deployment contract exists.
The default gate validates rejection; it does not claim release support.
"""
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

source_binary = Path(sys.argv[1]).resolve()
app_id = '00000000-0000-4000-8000-000000000003'
requests = []


class Platform(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        requests.append(self.path)
        payload = json.dumps({'id': app_id} if self.path.startswith('/applications/') else {'isActive': True}).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_POST(self):
        requests.append('UNEXPECTED POST')
        self.send_error(500)


server = ThreadingHTTPServer(('127.0.0.1', 0), Platform)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
account_name = 'generated-release-fixture-' + uuid.uuid4().hex
account_path = Path.home() / '.forklaunch/accounts' / f'{account_name}.toml'
account_path.parent.mkdir(parents=True, exist_ok=True)
with account_path.open('x') as token:
    token.write(f'access_token = "synthetic-generated-release-token"\nrefresh_token = ""\nexpires_at = {int(time.time())+3600}\n')
account_path.chmod(0o600)
root = Path(tempfile.mkdtemp(prefix='forklaunch-generated-release-')).resolve()
try:
    binary = root / 'forklaunch'
    shutil.copy2(source_binary, binary)
    print('CLI SHA-256:', hashlib.sha256(binary.read_bytes()).hexdigest(), flush=True)
    app = root / 'app'
    endpoint = f'http://127.0.0.1:{server.server_port}'
    env = {**os.environ, 'FORKLAUNCH_ACCOUNT': account_name, 'CI': 'true',
           'FORKLAUNCH_CLI': str(binary), 'FORKLAUNCH_SKIP_FORMAT': '1',
           'FORKLAUNCH_FRAMEWORK_SNAPSHOT': '1',
           'FORKLAUNCH_PLATFORM_MANAGEMENT_API_URL': endpoint,
           'FORKLAUNCH_BILLING_API_URL': endpoint}
    require_supported = os.environ.get('FORKLAUNCH_EXPECT_SHARED_RELEASE_SUPPORTED') == '1'
    guard = 'Shared language images require explicit process commands and migration dependencies that the release manifest contract cannot represent yet'

    def run(*args, cwd=None, **kwargs):
        print('Running:', shlex.join(map(str, args)), flush=True)
        return subprocess.run(list(map(str, args)), cwd=cwd or app, env=env,
                              check=kwargs.pop('check', True), timeout=1800, **kwargs)

    def cli(*args, path=None):
        return run(binary, *args, '--path', path or app, cwd=root)

    cli('init', 'application', 'release-app', '--database', 'postgresql', '--validator', 'zod',
        '--formatter', 'prettier', '--linter', 'eslint', '--http-framework', 'express',
        '--runtime', 'node', '--test-framework', 'vitest',
        '-D', 'Real release', '-A', 'ForkLaunch', '-L', 'MIT')
    cli('init', 'service', 'rust-api', '--language', 'rust', '-d', 'postgresql', '-D', 'Rust API')
    manifest = app / '.forklaunch/manifest.toml'
    manifest.write_text(f'platform_application_id = "{app_id}"\n' + manifest.read_text())
    assert (app / 'src/rust-api/main.rs').is_file()
    assert (app / 'src/rust-api/.forklaunch/contracts/Cargo.toml').is_file()
    assert not (app / 'sdks').exists()

    def release(prebuilt=False):
        args = [binary, 'release', 'create', '--path', app, '--local', '--dry-run',
                '--skip-sync', '--yes', '--version', '1.0.0']
        if prebuilt:
            args += ['--prebuilt-openapi', '.forklaunch/prebuilt', '--application-id', app_id]
        if require_supported:
            run(*args)
            return json.loads((app / '.forklaunch/release-manifest.json').read_text())
        result = run(*args, check=False, capture_output=True, text=True)
        output = result.stdout + result.stderr
        print(output, flush=True)
        assert result.returncode != 0, 'Shared-image release support changed: run the explicit positive capability matrix and update the platform contract evidence'
        assert guard in output, output
        assert not (app / '.forklaunch/release-manifest.json').exists(), 'Rejected release wrote an executable manifest'
        assert 'UNEXPECTED POST' not in requests, 'Rejected release published to the platform'
        assert 'synthetic-generated-release-token' not in output
        if prebuilt:
            assert 'Building Rust' not in output and 'Building TypeScript' not in output
        return None

    native = release()
    if native is not None:
        assert [service['name'] for service in native['services']] == ['rust-api']
        native_service = native['services'][0]
        assert native_service['runtime'] == 'tokio' and native_service['binary'] == 'rust-api'
        assert native_service['buildContext'] == '.' and native_service['dockerfile'] == 'Dockerfile.rust'
        assert '/health' in native_service['config']['openApiSpec']['paths']
    sdk = app / 'src/rust-api/sdk/typescript'
    contract = json.loads((sdk / 'contract.json').read_text())
    assert contract['generator'] == 'framework-rs' and contract['service']['name'] == 'rust-api'
    assert contract['formatVersion'] == 'forklaunch.contract.v1'
    assert (sdk / 'index.ts').read_text().strip() == "export type * from './sdk.js';"
    assert (app / 'src/rust-api/sdk/rust/generated/client.rs').is_file()
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    print('PASS raw optimized Rust build, local SDKs/OpenAPI and ' + ('supported release' if native else 'explicit shared-image release guard (capability remains blocked)'), flush=True)

    cli('init', 'service', 'ts-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'TypeScript API')
    ts = app / 'src/ts-api'
    cli('init', 'router', 'fixture', '--language', 'typescript', path=ts)
    assert ts.is_dir() and (app / 'src/rust-api').is_dir()
    assert all((app / ('Dockerfile.' + language)).is_file() for language in ['rust', 'typescript'])
    assert not (app / 'src/Dockerfile').exists()
    # Adding a package changes the lockfile; materialize it before the release's
    # frozen install, then exercise the generated build scripts unchanged.
    run('pnpm', 'install', '--ignore-scripts', '--no-frozen-lockfile')
    (app / '.forklaunch/release-manifest.json').unlink(missing_ok=True)
    mixed = release()
    if mixed is not None:
        specs = {service['name']: service['config']['openApiSpec'] for service in mixed['services']}
        assert set(specs) == {'rust-api', 'ts-api'}
        components = {service['name']: service for service in mixed['services']}
        assert components['rust-api']['runtime'] == 'tokio'
        assert components['rust-api']['binary'] == 'rust-api'
        assert 'runtime' not in components['ts-api'] and 'binary' not in components['ts-api']
    else:
        specs = {name: json.loads((app / path).read_text()) for name, path in {
            'rust-api': 'src/rust-api/sdk/typescript/openapi.json',
            'ts-api': 'src/ts-api/sdk/openapi.json',
        }.items()}
    for name, language in [('rust-api', 'rust'), ('ts-api', 'typescript')]:
        if mixed is not None:
            assert components[name]['buildContext'] == '.'
            assert components[name]['dockerfile'] == 'Dockerfile.' + language
        typescript_sdk = app / f'src/{name}/sdk/typescript/sdk.ts' if language == 'rust' else app / f'src/{name}/sdk.ts'
        assert typescript_sdk.is_file()
        if language == 'typescript':
            assert 'fixtureGet' in typescript_sdk.read_text()
            assert (app / f'src/{name}/dist/sdk.d.ts').is_file()
        assert (app / f'src/{name}/sdk/rust/Cargo.toml').is_file()
    export_controls = {'FORKLAUNCH_MODE', 'FORKLAUNCH_OPENAPI_OUTPUT', 'FORKLAUNCH_CONTRACT_OUTPUT', 'FORKLAUNCH_ASYNCAPI_OUTPUT'}
    if mixed is not None:
        required = {item['name'] for item in mixed.get('requiredEnvironmentVariables', [])}
        assert not export_controls & required, f'Export controls leaked into runtime requirements: {export_controls & required}'
    def exported_paths(spec):
        # TypeScript exports a version map; native services export one document.
        # Release/prebuilt preserve either representation verbatim.
        documents = [spec] if 'openapi' in spec else list(spec.values())
        assert documents and all(document.get('openapi', '').startswith('3.') and isinstance(document.get('paths'), dict) for document in documents)
        return {path for document in documents for path in document['paths']}

    assert '/health' in exported_paths(specs['rust-api'])
    assert any(path.startswith('/fixture') for path in exported_paths(specs['ts-api']))
    cli('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    print('PASS raw mixed Cargo/PNPM builds, local SDKs, Rust/tsx exports and ' + ('supported release' if mixed else 'explicit shared-image release guard (capability remains blocked)'), flush=True)

    for name, spec in specs.items():
        output = app / '.forklaunch/prebuilt' / name / 'openapi.json'
        output.parent.mkdir(parents=True)
        output.write_text(json.dumps(spec))
    # Prebuilt preparation must not compile or execute either language's source.
    (app / 'src/rust-api/main.rs').write_text('compile_error!("Prebuilt release must not compile application code");\n')
    (ts / 'server.ts').write_text('throw new Error("Prebuilt release must not execute application code");\n')
    prebuilt = release(True)
    if prebuilt is not None:
        assert {service['name']: service['config']['openApiSpec'] for service in prebuilt['services']} == specs
    else:
        assert all(json.loads((app / '.forklaunch/prebuilt' / name / 'openapi.json').read_text()) == spec for name, spec in specs.items())
    assert not any('synthetic-generated-release-token' in json.dumps(result) for result in [native, mixed, prebuilt])
    assert 'UNEXPECTED POST' not in requests, 'Dry run published to the platform'
    print('PASS prebuilt ' + ('artifact reuse' if prebuilt else 'shared-image rejection') + ' without source compilation/execution, secret leakage or publication', flush=True)
finally:
    account_path.unlink(missing_ok=True)
    server.shutdown()
    server.server_close()
    thread.join()
    if os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        print('Fixture retained:', root, flush=True)
    else:
        shutil.rmtree(root)
