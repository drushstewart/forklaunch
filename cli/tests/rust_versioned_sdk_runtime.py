#!/usr/bin/env python3
"""Versioned wire contracts exercised through current and legacy typed clients."""
import hashlib
from fixtures.legacy_sdk_manifest import write_legacy_sdk_manifest
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[2]
fixtures = repo / 'cli/tests/fixtures'
root = Path(tempfile.mkdtemp(prefix='forklaunch-versioned-sdk-')).resolve()
cli = root / 'forklaunch'
shutil.copy2(os.environ['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA256:', digest, flush=True)
app = root / 'original'
env = dict(os.environ)
passed = False
cases = []


def run(*args, cwd=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd or app, env=env, check=True, timeout=1800)


def command(*args, path=None):
    run(cli, *args, '--path', path or app, cwd=root)


def typescript(consumer, output):
    config = root / 'versioned-tsconfig.json'
    config.write_text(json.dumps({'compilerOptions': {
        'strict': True, 'skipLibCheck': True, 'target': 'ES2022', 'module': 'ESNext',
        'moduleResolution': 'Bundler', 'lib': ['ES2022', 'DOM'],
        'types': ['node'], 'typeRoots': [str(shared / 'node_modules/@types')],
        'rootDir': str(consumer.parent), 'outDir': str(output), 'noEmitOnError': True,
    }, 'files': [str(consumer)]}))
    run(os.environ.get('TSC_BIN', shared / 'node_modules/.bin/tsc'), '--project', config)


def rust_client(directory, modern=False, standalone=False):
    manifest = directory / 'Cargo.toml'
    source = manifest.read_text()
    if '[dev-dependencies]' not in source:
        source += '\n[dev-dependencies]\ntokio = { version="1", features=["macros","rt-multi-thread","time"] }\n'
    manifest.write_text(source)
    source = (fixtures / 'rust_versioned_sdk.rs').read_text()
    if modern or standalone:
        source = source.replace('VersionsVersions', '').replace('VersionsVersionedTransform', '')
        source = source.replace('versions_write_version', 'write_version').replace('versioned_transform_write_transform_version', 'write_transform_version')
    if modern:
        source = source.replace('VersionsClient::', 'Client::')
    example = directory / 'examples/version_roundtrip.rs'
    example.parent.mkdir(exist_ok=True)
    example.write_text(source)
    run('cargo', 'build', '--manifest-path', manifest, '--example', 'version_roundtrip')
    run('cargo', 'clippy', '--manifest-path', manifest, '--all-targets', '--', '-D', 'warnings')
    return Path(env['CARGO_TARGET_DIR']) / 'debug/examples/version_roundtrip'


def live(rust, consumer, label):
    run('node', repo / 'cli/tests/rust_versioned_sdk_runtime.mjs', binary, rust, consumer)
    cases.append({'client': label, 'passed': True})


try:
    command('init', 'application', 'versions-app', '--database', 'postgresql', '--validator', 'zod',
            '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express', '--runtime', 'node',
            '--test-framework', 'vitest', '-D', 'Versions', '-A', 'ForkLaunch', '-L', 'MIT')
    command('init', 'service', 'versions', '--language', 'rust', '-D', 'Versions')
    relocated = root / 'relocated'
    app.rename(relocated)
    app = relocated
    service = app / 'src/versions'
    fixture = (fixtures / 'rust_versioned_service.rs').read_text()
    start, end = fixture.index('pub fn contracts()'), fixture.index('pub fn register(')
    (service / 'api/controllers/version_contracts.rs').write_text(
        'use forklaunch_core::contract::{BodyContract, BodyKind, RequestContract, ResponseContract, RouteContract, VersionedContract};\nuse serde_json::json;\n' + fixture[start:end])
    (service / 'api/routes/version_fixture.rs').write_text(
        'use axum::{Extension,Json,http::StatusCode};\nuse forklaunch_axum::ForkLaunchRouter;\nuse forklaunch_core::http::SelectedVersion;\nuse serde_json::{Value,json};\nuse contracts::version_fixture::contracts;\n' + fixture[end:])
    host = service / '.forklaunch/contracts/lib.rs'
    host.write_text('#[path="../../api/controllers/version_contracts.rs"]\npub mod version_fixture;\n' +
        host.read_text().replace('//!', '//').replace('pub fn registry()', 'pub fn native_registry()').replace('pub fn binding_manifest()', 'pub fn native_binding_manifest()') +
        '\npub fn registry()->forklaunch_core::contract::ContractRegistry{let mut registry=native_registry();for route in version_fixture::contracts(){registry.register(route);}registry}\n' +
        'pub fn binding_manifest()->Result<Vec<forklaunch_core::contract::assembly::BindingDeclaration>,String>{let mut bindings=native_binding_manifest()?;for route in version_fixture::contracts(){bindings.push(forklaunch_core::contract::assembly::BindingDeclaration::http("api/controllers/version_contracts.rs","version_fixture",&route.operation_name,&route)?);}Ok(bindings)}\n')
    host_cargo = host.parent / 'Cargo.toml'
    host_cargo.write_text(host_cargo.read_text().replace('[dependencies]\n', '[dependencies]\nserde_json="1"\n'))
    routes = service / 'api/routes/mod.rs'
    routes.write_text('pub mod version_fixture;\n' + routes.read_text().replace('ContractRouter::new(contracts::registry())', 'ContractRouter::new(contracts::native_registry())'))
    cargo = service / 'Cargo.toml'
    cargo.write_text(cargo.read_text().replace('features = ["native-server"', 'features = ["native-server", "adapter"'))
    application = service / 'application.rs'
    application.write_text(application.read_text().replace('    Ok(router)', '''    let fixture = crate::api::routes::version_fixture::register(forklaunch_axum::ForkLaunchRouter::new(
        forklaunch_core::ServiceInfo::new("versions", env!("CARGO_PKG_VERSION")),
    ).with_config(forklaunch_axum::RouterConfig {openapi_enabled:false,docs_enabled:false,..Default::default()})).into_router();
    Ok(router.merge(fixture))'''))
    run('cargo', 'build', '--workspace')
    command('openapi', 'export')
    sdk = service / 'sdk/typescript'
    assert (sdk / 'index.ts').read_text().strip() == "export type * from './sdk.js';"
    artifact = json.loads((sdk / 'contract.json').read_text())
    scalar = next(route for route in artifact['routes'] if route['path'] == '/versioned-scalar')
    assert scalar['options']['bodyParser']['jsonStrict'] is False
    assert all('bodyParser' not in route['options'] for route in artifact['routes'] if route['path'] == '/versions')
    binary = Path(env['CARGO_TARGET_DIR']) / 'debug/versions'
    run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', binary)
    shared = root / 'universal-sdk'
    import sys; sys.path.insert(0, str(repo / 'cli/tests/fixtures')); from universal_sdk_source import copy_universal_sdk
    copy_universal_sdk(repo, shared)
    run('npm', 'install', '--prefix', shared, '--ignore-scripts', '--package-lock=false')
    run('npm', '--prefix', shared, 'run', 'build')
    dependency = app / 'node_modules/@forklaunch'
    dependency.mkdir(parents=True)
    (dependency / 'universal-sdk').symlink_to(shared, target_is_directory=True)
    original_ts = (fixtures / 'rust_versioned_sdk.ts').read_text()
    native_ts = app / 'version_fixture.ts'
    native_ts.write_text(original_ts.replace(original_ts.splitlines()[0],
        "import {universalSdk} from '@forklaunch/universal-sdk';\nimport {readFile} from 'node:fs/promises';\nimport type {VersionsSdkClient, WriteVersionV1Request as VersionsVersionsWriteVersionV1Request, WriteVersionV2Request as VersionsVersionsWriteVersionV2Request} from './src/versions/sdk/typescript/index.js';").replace(
        'const sdk=createForkLaunchSdk({baseUrl});', 'const sdk={versions:await universalSdk<VersionsSdkClient>({host:baseUrl,registryOptions:{raw:JSON.parse(await readFile(' + json.dumps(str(sdk / 'openapi.json')) + ",'utf8'))}})};"))
    for source in ['contract', 'openapi']:
        command('sdk', 'generate', '--target', 'all', '--from', source)
        command('sdk', 'generate', '--target', 'all', '--from', source, '--check')
        typescript(native_ts, app / 'native-dist')
        live(rust_client(service / 'sdk/rust', modern=True), app / 'native-dist/version_fixture.js', 'current-' + source)

    # Historical aggregate output remains a separately authored compatibility
    # input; current raw init is never expected to emit this retired structure.
    compat = root / 'legacy-compiler'
    artifacts = compat / '.forklaunch/contracts/versions'
    artifacts.mkdir(parents=True)
    write_legacy_sdk_manifest(app, compat / '.forklaunch/manifest.toml', '''[[projects]]
name = "versions"
type = "Service"
description = "Legacy compiler compatibility fixture"
routers = []
[projects.metadata]
language = "rust"
cratePath = "crates/versions"
contractArtifactPath = ".forklaunch/contracts/versions/contract.json"
openapiPath = ".forklaunch/contracts/versions/openapi.json"
''')
    artifact = json.loads((sdk / 'contract.json').read_text())
    for key in ['sdkCompiler', 'sdkLayout', 'typescriptRuntimeLayout', 'rustClientName', 'rustOutputLayout']:
        artifact.pop(key, None)
    (artifacts / 'contract.json').write_text(json.dumps(artifact))
    document = json.loads((sdk / 'openapi.json').read_text())
    for path in document['paths'].values():
        for operation in path.values():
            if isinstance(operation, dict):
                operation.pop('x-forklaunch-operation', None)
                operation.pop('x-forklaunch-operations', None)
    (artifacts / 'openapi.json').write_text(json.dumps(document))
    (compat / 'package.json').write_text('{"name":"legacy-versioned-fixture","type":"module","private":true}')
    run('cargo', 'run', '--manifest-path', repo / 'framework-rs/Cargo.toml', '-p', 'forklaunch-core', '--features', 'build,asyncapi-import', '--example', 'typescript_versioned_client', '--', artifacts / 'contract.json', root / 'named-sdk.ts', root / 'named-openapi.json')
    for source in ['contract', 'openapi', 'named-openapi']:
        if source == 'named-openapi':
            shutil.copy2(root / 'named-openapi.json', artifacts / 'openapi.json')
        source_arg = 'openapi' if source == 'named-openapi' else source
        command('sdk', 'generate', '--target', 'all', '--from', source_arg, path=compat)
        command('sdk', 'generate', '--target', 'all', '--from', source_arg, '--check', path=compat)
        if source == 'contract':
            run('npm', 'install', '--prefix', compat / 'sdks/typescript', '--ignore-scripts', '--package-lock=false')
        consumer = compat / 'sdks/typescript/src/version_fixture.ts'
        consumer.write_text(original_ts)
        typescript(consumer, compat / 'sdks/typescript/dist')
        live(rust_client(compat / 'sdks/rust/versions-sdk'), compat / 'sdks/typescript/dist/version_fixture.js', 'legacy-' + source)
    rust_dir = compat / 'sdks/rust/versions-sdk'
    run('cargo', 'run', '--manifest-path', repo / 'framework-rs/Cargo.toml', '-p', 'forklaunch-core', '--features', 'build,asyncapi-import', '--example', 'rust_sdk_versioned_client', '--', artifacts / 'contract.json', rust_dir / 'src/lib.rs')
    rust = rust_client(rust_dir, standalone=True)
    run('cargo', 'run', '--manifest-path', repo / 'framework-rs/Cargo.toml', '-p', 'forklaunch-core', '--features', 'build,asyncapi-import', '--example', 'typescript_versioned_client', '--', artifacts / 'contract.json', compat / 'sdks/typescript/src/index.ts')
    typescript(consumer, compat / 'sdks/typescript/dist')
    live(rust, compat / 'sdks/typescript/dist/version_fixture.js', 'framework-compilers')
    if os.environ.get('FORKLAUNCH_VERSIONED_SDK_RECEIPT'):
        Path(os.environ['FORKLAUNCH_VERSIONED_SDK_RECEIPT']).write_text(json.dumps({
            'schemaVersion': 1, 'cliSha256': digest, 'passed': True, 'cases': cases,
            'checks': ['neutral raw scaffold and relocation', 'compile-generated local SDKs', 'HTTP/MCP discovery and invocation',
                       'version fallback', 'body parser limits, charsets and compression', 'multipart and binary',
                       'hook verification and redaction', 'both typed clients', 'three artifact forms', 'graceful shutdown'],
        }, indent=2) + '\n')
    passed = True
finally:
    if passed and not os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
