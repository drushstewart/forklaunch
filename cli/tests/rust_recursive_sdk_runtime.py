#!/usr/bin/env python3
"""Current recursive contracts plus explicit legacy/native compiler conformance.

The component-name and resource-dialect gates select the same complete matrix.
Their original schemas, wire assertions and legacy typed consumers are retained.
"""
from fixtures.legacy_sdk_manifest import write_legacy_sdk_manifest
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

repo = Path(sys.argv[1]).resolve()
fixtures = repo / 'cli/tests/fixtures'
root = Path(tempfile.mkdtemp(prefix='forklaunch-recursive-sdk-')).resolve()
cli = root / 'forklaunch'
shutil.copy2(os.environ['FORKLAUNCH_CLI'], cli)
env = dict(os.environ)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA-256:', digest, flush=True)
runtime = root / 'typescript-runtime'
app = root
passed = False


def run(*args, cwd=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd or app, env=env, check=True, timeout=1800)


def command(*args, path=None):
    run(cli, *args, '--path', path or app, cwd=root)


def typescript(paths, output=None):
    options = {'strict': True, 'skipLibCheck': True, 'target': 'ES2022',
               'module': 'ESNext', 'moduleResolution': 'Bundler', 'lib': ['ES2022', 'DOM'],
               'types': ['node'], 'typeRoots': [str(runtime / 'node_modules/@types')]}
    if output:
        options.update(outDir=str(output), rootDir=os.path.commonpath([str(Path(p).parent) for p in paths]), noEmitOnError=True)
    else:
        options['noEmit'] = True
    config = root / 'recursive-tsconfig.json'
    config.write_text(json.dumps({'compilerOptions': options, 'files': list(map(str, paths))}))
    run(os.environ.get('TSC_BIN', runtime / 'node_modules/.bin/tsc'), '--project', config)


def rust_client(directory, source, name):
    manifest = directory / 'Cargo.toml'
    text = manifest.read_text()
    if '[dev-dependencies]' not in text:
        text += '\n[dev-dependencies]\ntokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }\n'
    manifest.write_text(text)
    (directory / 'examples').mkdir(exist_ok=True)
    (directory / 'examples' / (name + '.rs')).write_text(source)
    run('cargo', 'build', '--manifest-path', manifest, '--example', name)
    run('cargo', 'clippy', '--manifest-path', manifest, '--all-targets', '--', '-D', 'warnings')
    return Path(env['CARGO_TARGET_DIR']) / 'debug/examples' / name


def verify(binary, rust, typescript):
    run('node', repo / 'cli/tests/rust_recursive_sdk_runtime.mjs', binary, rust, typescript)


try:
    modes = os.environ.get('FORKLAUNCH_SCHEMA_RESOURCE_MODES', '').split()
    if not modes:
        modes = [os.environ.get('FORKLAUNCH_TEST_SCHEMA_RESOURCES', '')]
    for mode in modes:
        if mode:
            env['FORKLAUNCH_TEST_SCHEMA_RESOURCES'] = mode
        else:
            env.pop('FORKLAUNCH_TEST_SCHEMA_RESOURCES', None)
        case = root / (mode or 'default')
        case.mkdir()
        app = case / 'original'
        command('init', 'application', 'trees-app', '--database', 'postgresql', '--validator', 'zod',
                '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express', '--runtime', 'node',
                '--test-framework', 'vitest', '-D', 'Trees', '-A', 'ForkLaunch', '-L', 'MIT')
        command('init', 'service', 'trees', '--language', 'rust', '-D', 'Trees')
        relocated = case / 'relocated-app'
        shutil.copytree(app, relocated)
        shutil.rmtree(app)
        app = relocated
        service = app / 'src/trees'
        fixture = (fixtures / 'rust_recursive_service.rs').read_text()
        declarations = fixture[:fixture.index('pub fn tree()')].replace('use axum::Json;\n', '').replace('use forklaunch_axum::ForkLaunchRouter;\n', '')
        (service / 'api/controllers/recursive_contracts.rs').write_text(declarations)
        handlers = 'use axum::Json;\nuse forklaunch_axum::ForkLaunchRouter;\nuse serde_json::{Value,json};\nuse contracts::recursive_fixture::contracts;\n' + fixture[fixture.index('pub fn tree()'):]
        (service / 'api/routes/recursive_fixture.rs').write_text(handlers)
        host = service / '.forklaunch/contracts/lib.rs'
        host.write_text('#[path="../../api/controllers/recursive_contracts.rs"]\npub mod recursive_fixture;\n' + host.read_text().replace('//!', '//').replace('pub fn registry()', 'pub fn native_registry()').replace('pub fn binding_manifest()', 'pub fn native_binding_manifest()') + '\npub fn registry()->forklaunch_core::contract::ContractRegistry{let mut registry=native_registry();for route in recursive_fixture::contracts(){registry.register(route);}registry}\npub fn binding_manifest()->Result<Vec<forklaunch_core::contract::assembly::BindingDeclaration>,String>{let mut declarations=native_binding_manifest()?;for route in recursive_fixture::contracts(){declarations.push(forklaunch_core::contract::assembly::BindingDeclaration::http("api/controllers/recursive_contracts.rs","recursive_fixture",&route.operation_name,&route)?);}Ok(declarations)}\n')
        # These authored conformance schemas vary JSON resources at compile
        # time; the declaration-only host needs that direct fixture dependency.
        host_cargo = host.parent / 'Cargo.toml'
        host_cargo.write_text(host_cargo.read_text().replace('[dependencies]\n', '[dependencies]\nserde_json = "1"\n'))
        routes = service / 'api/routes/mod.rs'
        routes.write_text('pub mod recursive_fixture;\n' + routes.read_text().replace('ContractRouter::new(contracts::registry())', 'ContractRouter::new(contracts::native_registry())'))
        cargo = service / 'Cargo.toml'
        cargo.write_text(cargo.read_text().replace('features = ["native-server"', 'features = ["native-server", "adapter"'))
        application = service / 'application.rs'
        application.write_text(application.read_text().replace('    Ok(router)', '''    let fixture = forklaunch_axum::ForkLaunchRouter::from_local_router(
        forklaunch_core::ServiceInfo::new("trees", env!("CARGO_PKG_VERSION")),
        Vec::new(), forklaunch_core::http::LocalRouter::new(),
    );
    let fixture = crate::api::routes::recursive_fixture::register(fixture)
        .with_config(forklaunch_axum::RouterConfig { openapi_enabled: false, docs_enabled: false, ..Default::default() }).into_router();
    Ok(router.merge(fixture))'''))
        run('cargo', 'build', '--workspace')
        sdk = service / 'sdk/typescript'
        artifact = json.loads((sdk / 'contract.json').read_text())
        assert {r['operationName'] for r in artifact['routes']} == {'getHealth', 'readTree', 'writeTree', 'readForest', 'readChoice', 'readMutual'}
        assert (sdk / 'index.ts').read_text().strip() == "export type * from './sdk.js';"
        assert not (app / 'sdks').exists()
        command('openapi', 'export')
        if os.environ.get('FORKLAUNCH_TEST_COMPONENT_NAMES') == '1':
            document = json.loads((sdk / 'openapi.json').read_text())
            components = document['components']['schemas']
            assert all(re.fullmatch(r'[a-zA-Z0-9._-]+', name) for name in components)
            encoded = [name for name in components if name.startswith('_fl_encoded_')]
            assert len(encoded) == 4, encoded
            assert len({document['paths'][path]['get']['responses']['200']['content']['application/json']['schema']['$ref'] for path in ['/trees', '/forests', '/choices', '/mutual']}) == 4
            print('PASS valid collision-safe OpenAPI component names', flush=True)
        binary = Path(env['CARGO_TARGET_DIR']) / 'debug/trees'
        run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', binary)
        if not runtime.exists():
            import sys; sys.path.insert(0, str(repo / 'cli/tests/fixtures')); from universal_sdk_source import copy_universal_sdk
            copy_universal_sdk(repo, runtime)
            run('npm', 'install', '--prefix', runtime, '--ignore-scripts', '--package-lock=false')
            run('npm', '--prefix', runtime, 'run', 'build')
            run('node', repo / 'framework/universal-sdk/__test__/publicTransport.mjs', runtime)
        dependency = app / 'node_modules/@forklaunch'
        dependency.mkdir(parents=True)
        (dependency / 'universal-sdk').symlink_to(runtime, target_is_directory=True)
        native_ts = app / 'recursive_fixture.ts'
        ts_source = (fixtures / 'rust_recursive_sdk.ts').read_text().replace(
            "import { createForkLaunchSdk, type TreesTreesWriteTreeRequest } from './index.js';",
            "import {universalSdk} from '@forklaunch/universal-sdk';\nimport {readFile} from 'node:fs/promises';\nimport type {TreesSdkClient, WriteTreeRequest as TreesTreesWriteTreeRequest} from './src/trees/sdk/typescript/index.js';")
        ts_source = ts_source.replace('const sdk = createForkLaunchSdk({ baseUrl });', 'const sdk = {trees: await universalSdk<TreesSdkClient>({host:baseUrl,registryOptions:{raw:JSON.parse(await readFile(' + json.dumps(str(sdk / 'openapi.json')) + ",'utf8'))}})};")
        native_ts.write_text(ts_source)
        for source in ['contract', 'openapi']:
            command('sdk', 'generate', '--target', 'all', '--from', source)
            command('sdk', 'generate', '--target', 'all', '--from', source, '--check')
            typescript([native_ts], app / 'native-dist')
            rust = rust_client(service / 'sdk/rust', (fixtures / 'rust_recursive_sdk_native.rs').read_text(), 'native_recursive_roundtrip')
            verify(binary, rust, app / 'native-dist/recursive_fixture.js')
            print(f'PASS current recursive clients from {source}, resource mode {mode or "default"}', flush=True)

        # Authored legacy artifact input is an explicit compatibility boundary;
        # it is never represented as the output of current raw scaffolding.
        compat = case / 'legacy-compiler'
        (compat / '.forklaunch').mkdir(parents=True)
        write_legacy_sdk_manifest(app, compat / '.forklaunch/manifest.toml', '''[[projects]]
name = "trees"
type = "Service"
description = "Legacy recursive compiler compatibility fixture"
routers = []
[projects.metadata]
language = "rust"
cratePath = "crates/trees"
contractArtifactPath = ".forklaunch/contracts/trees/contract.json"
openapiPath = ".forklaunch/contracts/trees/openapi.json"
''')
        artifacts = compat / '.forklaunch/contracts/trees'
        artifacts.mkdir(parents=True)
        artifact = json.loads((sdk / 'contract.json').read_text())
        for key in ['sdkCompiler', 'sdkLayout', 'typescriptRuntimeLayout', 'rustClientName', 'rustOutputLayout']:
            artifact.pop(key, None)
        (artifacts / 'contract.json').write_text(json.dumps(artifact))
        document = json.loads((sdk / 'openapi.json').read_text())
        for path in document['paths'].values():
            for operation in path.values():
                if isinstance(operation, dict):
                    operation.pop('x-forklaunch-operation', None)
        (artifacts / 'openapi.json').write_text(json.dumps(document))
        (compat / 'package.json').write_text('{"name":"legacy-recursive-fixture","type":"module","private":true}')
        for source in ['contract', 'openapi']:
            command('sdk', 'generate', '--target', 'all', '--from', source, path=compat)
            command('sdk', 'generate', '--target', 'all', '--from', source, '--check', path=compat)
            if source == 'contract':
                run('npm', 'install', '--prefix', compat / 'sdks/typescript', '--ignore-scripts', '--package-lock=false')
            consumer = compat / 'sdks/typescript/src/recursive_fixture.ts'
            shutil.copy2(fixtures / 'rust_recursive_sdk.ts', consumer)
            typescript([consumer], compat / 'sdks/typescript/dist')
            rust = rust_client(compat / 'sdks/rust/trees-sdk', (fixtures / 'rust_recursive_sdk.rs').read_text(), 'legacy_recursive_roundtrip')
            verify(binary, rust, compat / 'sdks/typescript/dist/recursive_fixture.js')
            print(f'PASS legacy recursive compiler from {source}, resource mode {mode or "default"}', flush=True)

        # Preserve the independent public framework compiler APIs as well.
        rust_dir = compat / 'sdks/rust/trees-sdk'
        run('cargo', 'run', '--manifest-path', repo / 'framework-rs/Cargo.toml', '-p', 'forklaunch-core', '--features', 'build,asyncapi-import', '--example', 'rust_sdk_recursive_client', '--', artifacts / 'contract.json', rust_dir / 'src/lib.rs')
        source = (fixtures / 'rust_recursive_sdk.rs').read_text()
        for prefix in ['Trees', 'Forests', 'Choices', 'Mutual']:
            source = source.replace('Trees' + prefix, prefix)
        rust = rust_client(rust_dir, source, 'legacy_recursive_roundtrip')
        run('cargo', 'run', '--manifest-path', repo / 'framework-rs/Cargo.toml', '-p', 'forklaunch-core', '--features', 'build,asyncapi-import', '--example', 'typescript_recursive_client', '--', artifacts / 'contract.json', compat / 'sdks/typescript/src/index.ts')
        typescript([consumer], compat / 'sdks/typescript/dist')
        verify(binary, rust, compat / 'sdks/typescript/dist/recursive_fixture.js')
        print(f'PASS recursive framework compiler APIs, resource mode {mode or "default"}', flush=True)
        if not env.get('FORKLAUNCH_KEEP_FIXTURE'):
            # Each completed case has stopped its owned peers. Keep only the
            # shared installed runtime while the next resource dialect runs.
            shutil.rmtree(case)
    print('PASS complete recursive schema/SDK matrix with relocated current application and explicit legacy artifacts', flush=True)
    if env.get('FORKLAUNCH_RECURSIVE_SDK_RECEIPT'):
        Path(env['FORKLAUNCH_RECURSIVE_SDK_RECEIPT']).write_text(json.dumps({
            'status': 'PASS', 'cliSha256': digest,
            'schemaResourceModes': modes,
            'componentNames': env.get('FORKLAUNCH_TEST_COMPONENT_NAMES') == '1',
            'checks': ['current contract and OpenAPI typed TS/Rust clients',
                       'explicit legacy contract and OpenAPI typed TS/Rust clients',
                       'independent framework compiler APIs', 'strict generated Rust clippy',
                       'original recursive wire assertions and negative TypeScript typing'],
        }, indent=2) + '\n')
    passed = True
finally:
    if passed and not os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
