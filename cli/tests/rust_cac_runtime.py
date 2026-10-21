#!/usr/bin/env python3
"""Raw CAC module lifecycle plus unchanged authentication and wire assertions."""
from fixtures.rust_builtin_runtime_support import raw_application, build_public_runtime, typecheck, rust_consumer
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib

repo = Path(sys.argv[1]).resolve()
root = Path(tempfile.mkdtemp(prefix='forklaunch-cac-runtime-')).resolve()
cli = root / 'forklaunch'
shutil.copy2(os.environ['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA-256:', digest, flush=True)
env = dict(os.environ)
app = root / 'app'
passed = False


def run(*args, cwd=None, failure=None, extra_env=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    result = subprocess.run(list(map(str, args)), cwd=cwd or app, env={**env, **(extra_env or {})},
                            check=False, timeout=1800)
    assert (result.returncode != 0) if failure else (result.returncode == 0), result.returncode


def raw(destination, name, **options):
    raw_application(lambda *args: run(*args, cwd=root), cli, destination, name, **options)


def command(*args, path=None, **options):
    run(cli, *args, '--path', path or app, cwd=root, **options)


def sdk_paths(name):
    return [app / 'src' / name / 'sdk/rust', app / 'src' / name / 'sdk/typescript',
            app / 'src' / name / '.forklaunch/contracts']


try:
    raw(root / 'dry-app', 'dry-cac', dryrun=True)
    assert not (root / 'dry-app').exists()
    raw(root / 'module-first', 'cac-first')
    command('init', 'module', '--language', 'rust', '-m', 'cac-base', path=root / 'module-first')
    assert (root / 'module-first/src/cac/main.rs').is_file()
    run('cargo', 'metadata', '--manifest-path', root / 'module-first/Cargo.toml', '--no-deps', '--format-version', '1', cwd=root)
    raw(app, 'cac-runtime')
    command('init', 'module', 'dry-cac', '--language', 'rust', '-m', 'cac-base', '--dryrun')
    assert not (app / 'src/dry-cac').exists()
    command('init', 'module', 'iam', '--language', 'rust', '-m', 'iam-base', '-d', 'postgresql')
    blueprint = app / '.forklaunch/blueprint-rs/Cargo.toml'
    source = blueprint.read_text()
    blueprint.write_text('\n'.join(line for line in source.splitlines() if '/cac/' not in line and 'cac-interface' not in line) + '\n# user-workspace-marker\n')
    command('init', 'module', '--language', 'rust', '-m', 'cac-base')
    relocated = root / 'relocated-app'
    app.rename(relocated)
    app = relocated
    assert (app / 'src/cac/sdk/rust/.forklaunch-sdk-local').is_file()
    assert (app / 'src/cac/.forklaunch/contracts/Cargo.toml').is_file()
    assert not (app / 'src/client-sdk/typescript/cac').exists()
    blueprint = app / '.forklaunch/blueprint-rs/Cargo.toml'
    assert 'user-workspace-marker' in blueprint.read_text()
    members = tomllib.loads(blueprint.read_text())['workspace']['members']
    for member in ['crates/interfaces/cac-interface', 'crates/implementations/cac/cac-base']:
        assert members.count(member) == 1, (member, members)
    # The HTTP layer is application source, not a blueprint crate.
    assert 'crates/implementations/cac/cac-axum' not in members
    assert (app / 'src/cac/api/http/mod.rs').is_file()
    library = app / 'src/cac/lib.rs'
    library.write_text(library.read_text() + '\npub const USER_MODULE_MARKER: &str = "preserved";\n')
    manifest = app / 'src/cac/Cargo.toml'
    manifest.write_text(manifest.read_text() + '\n[package.metadata.user]\nmarker = "preserved"\n')
    command('sync', 'service', 'cac')
    command('init', 'module', 'cac', '--language', 'rust', '-m', 'cac-base')
    command('init', 'service', 'cac', '--language', 'rust', '-d', 'postgresql', '-D', 'Invalid', failure=True)
    assert 'USER_MODULE_MARKER' in library.read_text()
    assert 'CAC_HMAC_SECRET_KEY' in (app / 'docker-compose.yaml').read_text()
    run('cargo', 'build', '--workspace')
    command('openapi', 'export', extra_env={'OTEL_EXPORTER_OTLP_ENDPOINT': 'invalid', 'OTEL_EXPORTER_OTLP_PROTOCOL': 'invalid'})
    command('sdk', 'generate', '--target', 'all', '--from', 'contract')
    sdk = app / 'src/cac/sdk/typescript'
    assert (sdk / 'index.ts').read_text().strip() == "export type * from './sdk.js';"
    contract = json.loads((sdk / 'contract.json').read_text())
    assert len(contract['routes']) == 5
    assert {route['path'] for route in contract['routes']} == {'/codeSet','/codeSet/:code','/compliance/export/:userId','/compliance/erase/:userId','/health'}
    runtime = root / 'typescript-runtime'
    entry = build_public_runtime(run, app, runtime, repo)
    usage = app / 'cac-typecheck.ts'
    usage.write_text('''import { universalSdk } from '@forklaunch/universal-sdk';
import type { CacSdkClient } from './src/cac/sdk/typescript/index.js';
async function check() {
 const sdk = await universalSdk<CacSdkClient>({host:'https://cac.example',registryOptions:{path:'openapi.json'}});
 const found = await sdk.codeSet.lookupProcedureCode({params:{code:'PROC-003'}});
 if (found.code === 200) { const description:string = found.response.description; void description; }
 // @ts-expect-error The path code is a string.
 await sdk.codeSet.lookupProcedureCode({params:{code:42}});
}
void check;
''')
    typecheck(run, app, runtime, [app / 'src/cac/sdk/typescript/index.ts', usage])
    source = (repo / 'cli/tests/fixtures/rust_cac_sdk_runtime.rs').read_text()
    consumer = rust_consumer(app, root / 'rust-consumer', 'cac', package='cac-sdk', test_source=source)
    run('cargo', 'test', '--manifest-path', consumer, '--test', 'runtime', '--no-run')
    run('cargo', 'check', '--workspace')
    binary = Path(env['CARGO_TARGET_DIR']) / 'debug/cac'
    run('node', repo / 'cli/tests/rust_cac_runtime.mjs', binary, entry, consumer)
    command('change', 'service', '-N', 'coding', '--confirm', path=app / 'src/cac')
    assert 'USER_MODULE_MARKER' in (app / 'src/coding/lib.rs').read_text()
    assert 'marker = "preserved"' in (app / 'src/coding/Cargo.toml').read_text()
    project = next(p for p in tomllib.loads((app / '.forklaunch/manifest.toml').read_text())['projects'] if p['name'] == 'coding')
    assert project['variant'] == 'cac-base'
    command('sync', 'service', 'coding')
    command('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    run('cargo', 'check', '--workspace')
    command('delete', 'service', 'coding', '--continue')
    assert not (app / 'src/coding').exists()
    assert all(not path.exists() for path in sdk_paths('coding'))
    assert not (app / '.forklaunch/contracts/coding').exists()
    print('PASS raw CAC module initialization, relocation, blueprint upgrade, source preservation, offline artifacts, typed SDKs, HMAC/JWKS/RBAC runtime and lifecycle', flush=True)
    if env.get('FORKLAUNCH_CAC_RECEIPT'):
        Path(env['FORKLAUNCH_CAC_RECEIPT']).write_text(json.dumps({
            'status':'PASS','cliSha256':digest,
            'registryDiscovery':{'path':'openapi.json','convention':'Relative path, matching blueprint IAM surfacing.ts clients and framework/universal-sdk/src/core/openApi.ts; actual hash/document discovery remains enabled'},
            'wireAssertions':'Authentication rejection, tenant/RBAC, SDK result, MCP and shutdown assertions',
            'invalidJwtStatus':{
                'status':403,
                'cases':['wrong issuer','wrong audience','expired','future not-before'],
                'body':'Exact sanitized forbidden/invalid credentials with correlation ID; no extra fields',
                'policySources':[
                    'framework/core/src/http/middleware/request/auth.middleware.ts: invalidAuthorizationToken',
                    'framework-rs/crates/axum/src/router.rs: authentication_error',
                    'framework-rs/crates/axum/src/session.rs: rejected_jwt_status_matches_typescript_without_changing_opaque_sessions',
                ],
                'preserved':'Missing credentials and invalid HMAC remain 401; denied system-role authorization remains 403',
            },
        },indent=2)+'\n')
    passed = True
finally:
    if passed and not env.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
