#!/usr/bin/env python3
"""Versioned field hooks on a current generated app, retaining exact wire cases."""
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[2]
root = Path(tempfile.mkdtemp(prefix='forklaunch-versioned-fields-')).resolve()
cli = root / 'forklaunch'
shutil.copy2(os.environ['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA256:', digest, flush=True)
app = root / 'original'
env = dict(os.environ)
passed = False


def run(*args, cwd=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd or app, env=env, check=True, timeout=1800)


def command(*args, path=None):
    run(cli, *args, '--path', path or app, cwd=root)


try:
    command('init', 'application', 'fields-app', '--database', 'postgresql', '--validator', 'zod',
            '--formatter', 'prettier', '--linter', 'oxlint', '--http-framework', 'express', '--runtime', 'node',
            '--test-framework', 'vitest', '-D', 'Fields', '-A', 'ForkLaunch', '-L', 'MIT')
    command('init', 'service', 'fields', '--language', 'rust', '-D', 'Fields')
    relocated = root / 'relocated'
    app.rename(relocated)
    app = relocated
    service = app / 'src/fields'
    fixture = (repo / 'cli/tests/fixtures/rust_version_fields.rs').read_text()
    start, end = fixture.index('fn field('), fixture.index('pub fn register(')
    declarations = '''use forklaunch_core::contract::{BodyContract, FieldContract, RequestContract, ResponseContract, RouteContract, VersionedContract};
use serde_json::{Value,json};
''' + fixture[start:end]
    (service / 'api/controllers/field_contracts.rs').write_text(declarations)
    runtime = fixture[:fixture.index('use forklaunch_core::contract')]
    runtime += fixture[fixture.index('use forklaunch_core::http'):start]
    runtime += 'use contracts::fields_fixture::contracts;\n' + fixture[end:]
    (service / 'api/routes/fields_fixture.rs').write_text(runtime)
    host = service / '.forklaunch/contracts/lib.rs'
    host.write_text('#[path="../../api/controllers/field_contracts.rs"]\npub mod fields_fixture;\n' +
        host.read_text().replace('//!', '//').replace('pub fn registry()', 'pub fn native_registry()').replace('pub fn binding_manifest()', 'pub fn native_binding_manifest()') +
        '\npub fn registry()->forklaunch_core::contract::ContractRegistry{let mut registry=native_registry();for route in fields_fixture::contracts(){registry.register(route);}registry}\n' +
        'pub fn binding_manifest()->Result<Vec<forklaunch_core::contract::assembly::BindingDeclaration>,String>{let mut bindings=native_binding_manifest()?;for route in fields_fixture::contracts(){bindings.push(forklaunch_core::contract::assembly::BindingDeclaration::http("api/controllers/field_contracts.rs","fields_fixture",&route.operation_name,&route)?);}Ok(bindings)}\n')
    host_cargo = host.parent / 'Cargo.toml'
    host_cargo.write_text(host_cargo.read_text().replace('[dependencies]\n', '[dependencies]\nserde_json = "1"\n'))
    routes = service / 'api/routes/mod.rs'
    routes.write_text('pub mod fields_fixture;\n' + routes.read_text().replace('ContractRouter::new(contracts::registry())', 'ContractRouter::new(contracts::native_registry())'))
    cargo = service / 'Cargo.toml'
    cargo.write_text(cargo.read_text().replace('features = ["native-server"', 'features = ["native-server", "adapter"'))
    application = service / 'application.rs'
    application.write_text(application.read_text().replace('    Ok(router)', '''    let fixture = crate::api::routes::fields_fixture::register(forklaunch_axum::ForkLaunchRouter::new(
        forklaunch_core::ServiceInfo::new("fields", env!("CARGO_PKG_VERSION")),
    ).with_config(forklaunch_axum::RouterConfig { openapi_enabled: false, docs_enabled: false, ..Default::default() })).into_router();
    Ok(router.merge(fixture))'''))
    run('cargo', 'build', '--workspace')
    command('openapi', 'export')
    sdk = service / 'sdk/typescript'
    assert (sdk / 'index.ts').read_text().strip() == "export type * from './sdk.js';"
    # Install the generated runtime dependency so the real type-only package
    # resolves without a repository node_modules assumption.
    shared = root / 'universal-sdk'
    import sys; sys.path.insert(0, str(repo / 'cli/tests/fixtures')); from universal_sdk_source import copy_universal_sdk
    copy_universal_sdk(repo, shared)
    run('npm', 'install', '--prefix', shared, '--ignore-scripts', '--package-lock=false')
    run('npm', '--prefix', shared, 'run', 'build')
    dependencies = app / 'node_modules/@forklaunch'
    dependencies.mkdir(parents=True)
    (dependencies / 'universal-sdk').symlink_to(shared, target_is_directory=True)
    tsc = Path(os.environ.get('TSC_BIN', shared / 'node_modules/.bin/tsc'))
    for source in ['contract', 'openapi']:
        command('sdk', 'generate', 'all', '--from', source)
        command('sdk', 'generate', 'all', '--from', source, '--check')
        run(tsc, '--project', sdk / 'tsconfig.json', '--noEmit')
        run('cargo', 'check', '--manifest-path', service / 'sdk/rust/Cargo.toml')
    binary = Path(env['CARGO_TARGET_DIR']) / 'debug/fields'
    run('node', repo / 'cli/tests/rust_versioned_fields_runtime.mjs', binary)
    run('node', repo / 'cli/tests/rust_mcp_service_startup.mjs', binary)
    if os.environ.get('FORKLAUNCH_VERSIONED_FIELDS_RECEIPT'):
        Path(os.environ['FORKLAUNCH_VERSIONED_FIELDS_RECEIPT']).write_text(json.dumps({
            'schemaVersion': 1, 'passed': True, 'cliSha256': digest,
            'checks': ['neutral raw application', 'relocated current native service', 'compile-triggered local SDKs',
                       'contract and OpenAPI SDK generation/check', 'TypeScript and Rust compilation',
                       'original 36 HTTP/MCP field results', 'encoded and malformed path parameters',
                       'typed defaults, version fallback and missing outputs', 'wire header and query preservation',
                       'validation errors hide hook details', 'HTTP/MCP startup and graceful shutdown'],
        }, indent=2) + '\n')
    passed = True
finally:
    if passed and not os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
