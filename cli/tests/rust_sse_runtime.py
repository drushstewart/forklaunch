#!/usr/bin/env python3
"""Current raw scaffold and explicit legacy compiler clients share live SSE cases."""
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
root = Path(tempfile.mkdtemp(prefix='forklaunch-sse-runtime-')).resolve()
cli = root / 'forklaunch'
shutil.copy2(os.environ['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA-256:', digest, flush=True)
app = root / 'original'
env = dict(os.environ)
results = []
passed = False


def run(*args, cwd=None, extra=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd or app, env={**env, **(extra or {})}, check=True, timeout=1800)


def command(*args, path=None):
    run(cli, *args, '--path', path or app, cwd=root)


def typescript(source, output=None):
    options = {'strict': True, 'skipLibCheck': True, 'target': 'ES2022',
               'module': 'ESNext', 'moduleResolution': 'Bundler', 'lib': ['ES2022', 'DOM']}
    if output:
        options.update(outDir=str(output), rootDir=str(source.parent), noEmitOnError=True)
    else:
        options['noEmit'] = True
    config = root / 'fixture-tsconfig.json'
    config.write_text(json.dumps({'compilerOptions': options, 'files': [str(source)]}))
    run(tsc, '--project', config)


def rust_example(directory, modern=False, standalone=False):
    manifest = directory / 'Cargo.toml'
    text = manifest.read_text()
    if '[dev-dependencies]' not in text:
        text += '\n[dev-dependencies]\ntokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }\n'
    manifest.write_text(text)
    source = (fixtures / 'rust_sse_sdk.rs').read_text()
    if modern or standalone:
        prefix = 'EventsWatchEvents' if standalone else 'WatchEvents'
        source = source.replace('StreamsEventsWatchEvents', prefix)
        if modern:
            source = source.replace('StreamsClient', 'Client').replace('.events_watch_events(', '.watch_events(')
        source = source.replace('params: [("mode".into(), mode.into())].into(),',
                                f'params: streams_sdk::{prefix}Params {{ mode: mode.into() }},')
        # The current single-response enum is exhaustive, not a legacy fallback.
        if modern:
            source = re.sub(r'\s*else\s*\{\s*panic!\("stream missing"\)\s*\}', '', source)
    example = directory / 'examples/sse_roundtrip.rs'
    example.parent.mkdir(exist_ok=True)
    example.write_text(source)
    run('cargo', 'build', '--manifest-path', manifest, '--example', 'sse_roundtrip')
    run('cargo', 'clippy', '--manifest-path', manifest, '--lib', '--example', 'sse_roundtrip', '--', '-D', 'warnings')
    return Path(env['CARGO_TARGET_DIR']) / 'debug/examples/sse_roundtrip'


def live(driver, rust, label, cross=True):
    for fetcher in ['', str(cross_fetch)] if cross else ['']:
        run('node', repo / 'cli/tests/rust_sse_runtime.mjs', binary, rust, driver,
            extra={'FORKLAUNCH_TEST_FETCH_MODULE': fetcher})
        results.append({'client': label, 'fetch': 'cross-fetch' if fetcher else 'default', 'passed': True})


try:
    command('init', 'application', 'streams-app', '--database', 'postgresql', '--validator', 'zod',
            '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express', '--runtime', 'node',
            '--test-framework', 'vitest', '-D', 'Streams', '-A', 'ForkLaunch', '-L', 'MIT')
    command('init', 'service', 'streams', '--language', 'rust', '-D', 'Streams')
    relocated = root / 'relocated'
    app.rename(relocated)
    app = relocated
    service = app / 'src/streams'
    # Only malformed-wire fixture routes use the adapter. Generated native
    # bootstrap, health, docs, telemetry, security and MCP remain in place.
    fixture = (fixtures / 'rust_sse_service.rs').read_text()
    start, end = fixture.index('pub fn contracts()'), fixture.index('pub fn register(')
    declarations = 'use forklaunch_core::contract::{AccessLevel, BodyContract, BodyKind, FieldContract, RouteContract, SchemaRef};\n' + fixture[start:end]
    (service / 'api/controllers/sse_contracts.rs').write_text(declarations)
    handler = fixture[:start] + 'use contracts::sse_fixture::contracts;\n' + fixture[end:]
    handler = handler.replace('    AccessLevel, BodyContract, BodyKind, FieldContract, RouteContract, SchemaRef,', '    BodyContract, RouteContract,')
    handler = handler.replace('RouterConfig::default().with_authenticator', 'RouterConfig { openapi_enabled: false, docs_enabled: false, ..Default::default() }.with_authenticator')
    (service / 'api/routes/sse_fixture.rs').write_text(handler)
    host = service / '.forklaunch/contracts/lib.rs'
    host.write_text('#[path="../../api/controllers/sse_contracts.rs"]\npub mod sse_fixture;\n' +
        host.read_text().replace('//!', '//').replace('pub fn registry()', 'pub fn native_registry()').replace('pub fn binding_manifest()', 'pub fn native_binding_manifest()') +
        '\npub fn registry()->forklaunch_core::contract::ContractRegistry {let mut registry=native_registry();for route in sse_fixture::contracts(){registry.register(route);}registry}\n' +
        'pub fn binding_manifest()->Result<Vec<forklaunch_core::contract::assembly::BindingDeclaration>,String>{let mut bindings=native_binding_manifest()?;for route in sse_fixture::contracts(){bindings.push(forklaunch_core::contract::assembly::BindingDeclaration::http("api/controllers/sse_contracts.rs","sse_fixture", &route.operation_name, &route)?);}Ok(bindings)}\n')
    routes = service / 'api/routes/mod.rs'
    routes.write_text('pub mod sse_fixture;\n' + routes.read_text().replace('ContractRouter::new(contracts::registry())', 'ContractRouter::new(contracts::native_registry())'))
    manifest = service / 'Cargo.toml'
    manifest.write_text(manifest.read_text().replace('[dependencies]\n', '[dependencies]\nfutures-util = "0.3"\n').replace('features = ["native-server"]', 'features = ["native-server", "adapter", "sse"]'))
    application = service / 'application.rs'
    application.write_text(application.read_text().replace('    Ok(router)', '''    let fixture = crate::api::routes::sse_fixture::register(forklaunch_axum::ForkLaunchRouter::new(
        forklaunch_core::ServiceInfo::new("streams", env!("CARGO_PKG_VERSION")),
    )).into_router();
    Ok(router.merge(fixture))'''))
    run('cargo', 'build', '--workspace')
    sdk = service / 'sdk/typescript'
    assert (sdk / 'index.ts').read_text().strip() == "export type * from './sdk.js';"
    assert not (app / 'sdks').exists()
    command('openapi', 'export')
    binary = Path(env['CARGO_TARGET_DIR']) / 'debug/streams'
    run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', binary)
    runtime = root / 'typescript-runtime'
    import sys; sys.path.insert(0, str(repo / 'cli/tests/fixtures')); from universal_sdk_source import copy_universal_sdk
    copy_universal_sdk(repo, runtime)
    run('npm', 'install', '--prefix', runtime, '--ignore-scripts', '--package-lock=false')
    run('npm', '--prefix', runtime, 'run', 'build')
    run('node', repo / 'framework/universal-sdk/__test__/publicTransport.mjs', runtime)
    dependency = app / 'node_modules/@forklaunch'
    dependency.mkdir(parents=True)
    (dependency / 'universal-sdk').symlink_to(runtime, target_is_directory=True)
    tsc = Path(os.environ.get('TSC_BIN', runtime / 'node_modules/.bin/tsc'))
    cross_fetch = Path(os.environ.get('CROSS_FETCH_PATH', tsc.parent.parent / 'cross-fetch'))
    if not cross_fetch.is_dir():
        run('npm', 'install', '--prefix', runtime, '--ignore-scripts', '--package-lock=false', '--no-save', 'cross-fetch@4.1.0')
        cross_fetch = runtime / 'node_modules/cross-fetch'
    driver = root / 'native-sdk.mjs'
    driver.write_text('''import {readFile} from 'node:fs/promises';
import {universalSdk} from './typescript-runtime/lib/index.mjs';
export const strictContractResponses = true;
const document=JSON.parse(await readFile(new URL('./relocated/src/streams/sdk/typescript/openapi.json',import.meta.url),'utf8'));
export async function createForkLaunchSdk({baseUrl, ...options}={}) {
 return {streams: await universalSdk({host:baseUrl,registryOptions:{raw:document},...options})};
}
''')
    usage = app / 'stream-consumer.ts'
    usage.write_text('''import {universalSdk} from '@forklaunch/universal-sdk';
import type {StreamsSdkClient} from './src/streams/sdk/typescript/index.js';
async function consume(document: Parameters<typeof universalSdk>[0]['registryOptions']) {
 const client = await universalSdk<StreamsSdkClient>({host:'https://service.invalid', registryOptions:document});
 const result = await client.events.watchEvents({params:{mode:'finite'}});
 if (result.code === 200) for await (const event of result.response) { const id: string | undefined = event.id; void id; }
 // @ts-expect-error Required path parameter must be supplied.
 client.events.watchEvents({});
 // @ts-expect-error Unknown parameter is not a declared type member.
 client.events.watchEvents({params:{missing:'finite'}});
}
void consume;
''')
    for source in ['contract', 'openapi']:
        command('sdk', 'generate', 'all', '--from', source)
        typescript(usage)
        rust = rust_example(service / 'sdk/rust', modern=True)
        live(driver, rust, 'current-' + source)
    # Retain the historical compiler API as explicit versioned input. Current
    # CLI initialization must not manufacture a retired application layout.
    compat = root / 'legacy-compiler'
    artifacts = compat / '.forklaunch/contracts/streams'
    artifacts.mkdir(parents=True)
    write_legacy_sdk_manifest(app, compat / '.forklaunch/manifest.toml', '''[[projects]]
name = "streams"
type = "Service"
description = "Legacy compiler compatibility fixture"
routers = []
[projects.metadata]
language = "rust"
cratePath = "crates/streams"
contractArtifactPath = ".forklaunch/contracts/streams/contract.json"
openapiPath = ".forklaunch/contracts/streams/openapi.json"
''')
    contract = json.loads((sdk / 'contract.json').read_text())
    for key in ['sdkCompiler', 'sdkLayout', 'typescriptRuntimeLayout', 'rustClientName', 'rustOutputLayout']:
        contract.pop(key, None)
    (artifacts / 'contract.json').write_text(json.dumps(contract))
    openapi = json.loads((sdk / 'openapi.json').read_text())
    for path in openapi['paths'].values():
        for operation in path.values():
            if isinstance(operation, dict):
                operation.pop('x-forklaunch-operation', None)
                operation.pop('x-forklaunch-operations', None)
    (artifacts / 'openapi.json').write_text(json.dumps(openapi))
    (compat / 'package.json').write_text('{"name":"legacy-sse-fixture","type":"module","private":true}')
    for source in ['contract', 'openapi']:
        command('sdk', 'generate', 'all', '--from', source, path=compat)
        if source == 'contract':
            run('npm', 'install', '--prefix', compat / 'sdks/typescript', '--ignore-scripts', '--package-lock=false')
        typescript(compat / 'sdks/typescript/src/index.ts', compat / 'sdks/typescript/dist')
        rust = rust_example(compat / 'sdks/rust/streams-sdk')
        live(compat / 'sdks/typescript/dist/index.js', rust, 'legacy-' + source)
    rust_dir = compat / 'sdks/rust/streams-sdk'
    run('cargo', 'run', '--manifest-path', repo / 'framework-rs/Cargo.toml', '-p', 'forklaunch-core', '--features', 'build,asyncapi-import', '--example', 'rust_sdk_stream_client', '--', rust_dir / 'src/lib.rs')
    rust = rust_example(rust_dir, standalone=True)
    ts_source = compat / 'sdks/typescript/src/index.ts'
    for imported in [False, True]:
        if imported:
            (compat / 'sdks/typescript/node_modules/cross-fetch').symlink_to(cross_fetch, target_is_directory=True)
        run('cargo', 'run', '--manifest-path', repo / 'framework-rs/Cargo.toml', '-p', 'forklaunch-core', '--features', 'build,asyncapi-import', '--example', 'typescript_stream_client', '--', ts_source, *(['--import-fetch'] if imported else []))
        typescript(ts_source, compat / 'sdks/typescript/dist')
        live(compat / 'sdks/typescript/dist/index.js', rust, 'framework-imported-fetch' if imported else 'framework', cross=not imported)
    if os.environ.get('FORKLAUNCH_SSE_RECEIPT'):
        Path(os.environ['FORKLAUNCH_SSE_RECEIPT']).write_text(json.dumps({'schemaVersion': 1, 'cliSha256': digest, 'cases': results,
            'checks': ['raw neutral scaffold', 'relocation', 'compile-generated local types', 'strict TypeScript and Rust clients',
                       'HTTP/MCP', 'auth before work', 'Unicode and multiline frames', 'stream bounds', 'cancellation',
                       'EOF and transport failures', 'listener and stream cleanup', 'graceful shutdown'], 'passed': True}, indent=2) + '\n')
    passed = True
finally:
    if passed and not os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
