#!/usr/bin/env python3
"""Raw messaging/billing modules through real providers, storage, SDKs and lifecycle."""
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time

from fixtures.rust_builtin_runtime_support import (
    raw_application, build_public_runtime, rust_consumer, typecheck, sdk_type_inputs,
    contract_artifact,
)

repo = Path(__file__).resolve().parents[2]
family = os.environ['FORKLAUNCH_TEST_PROVIDER_MODULE']
assert family in ('messaging', 'billing')
provider = 'twilio' if family == 'messaging' else 'stripe'
root = Path(tempfile.mkdtemp(prefix=f'forklaunch-{family}-')).resolve()
app = root / 'original'
env = dict(os.environ)
cli = root / 'forklaunch'
shutil.copy2(env['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA256:', digest, flush=True)
container = f'forklaunch-{family}-{os.getpid()}'
env['FORKLAUNCH_TEST_ROLE'] = f'fl_{"sms" if family == "messaging" else "billing"}_{int(time.time())}_{os.getpid()}'
owned_database = False
role_created = False
passed = False


def run(*args, cwd=None, extra=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd or app, env={**env, **(extra or {})}, check=True, timeout=1800)


def command(*args, path=None, success=True):
    args = [str(cli), *args, '--path', str(path or app)]
    print('Running:', shlex.join(args), flush=True)
    result = subprocess.run(args, cwd=root, env=env, text=True, capture_output=True, timeout=300)
    print(result.stdout, end='', flush=True)
    assert (result.returncode == 0) == success, result.stdout + result.stderr
    return result


def snapshot():
    return {str(p.relative_to(app)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in app.rglob('*') if p.is_file()}


try:
    raw_application(lambda *args: run(*args, cwd=root), cli, root / 'dry', 'dry', dryrun=True)
    assert not (root / 'dry').exists()
    # Application presets remain removed. Rejection cannot create an application.
    command('init', 'application', 'invalid', '--modules', family + '-base',
            path=root / 'invalid', success=False)
    assert not (root / 'invalid').exists()
    raw_application(lambda *args: run(*args, cwd=root), cli, app, family + '-app')
    before = snapshot()
    command('init', 'module', 'dry-provider', '--language', 'rust', '-m', family + '-' + provider, '--dryrun')
    assert snapshot() == before
    command('init', 'module', family, '--language', 'rust', '-m', family + '-base')
    command('init', 'module', provider, '--language', 'rust', '-m', family + '-' + provider)
    # Supply the ordinary public runtime package through raw service scaffolding.
    command('init', 'service', 'runtime-host', '--language', 'rust', '-D', 'SDK runtime fixture')
    if family == 'billing':
        for feature in ['email', 'sms', 'whatsapp', 'voice', 'payments']:
            command('infra', 'add', provider, feature)
    app.rename(root / 'relocated')
    app = root / 'relocated'
    source = app / 'src' / provider
    marker = f'USER_{family.upper()}_MARKER'
    with (source / 'lib.rs').open('a') as file:
        file.write(f'\npub const {marker}: &str = "preserved";\n')
    with (source / 'Cargo.toml').open('a') as file:
        file.write('\n[package.metadata.user]\nmarker = "preserved"\n')
    command('sync', 'service', provider)
    command('init', 'module', provider, '--language', 'rust', '-m', family + '-' + provider)
    assert marker in (source / 'lib.rs').read_text()
    assert ('TWILIO_STATUS_CALLBACK_URL' if family == 'messaging' else 'STRIPE_SECRET_KEY') in (app / 'docker-compose.yaml').read_text()
    before = snapshot()
    command('init', 'service', provider, '--language', 'rust', '-d', 'postgresql', '-D', 'Invalid', success=False)
    assert snapshot() == before, 'Rejected generic service replacement changed the builtin'
    run('cargo', 'build', '--workspace')
    run(cli, 'openapi', 'export', '--path', app,
        extra={'OTEL_EXPORTER_OTLP_ENDPOINT': 'invalid', 'OTEL_EXPORTER_OTLP_PROTOCOL': 'invalid'})
    runtime = root / 'public-runtime'
    entry = build_public_runtime(run, app, runtime, repo)
    manifests = {}
    for origin in ['contract', 'openapi']:
        command('sdk', 'generate', '--target', 'all', '--from', origin)
        typecheck(run, app, runtime, sdk_type_inputs(app, [family, provider]))
        run('cargo', 'check', '--workspace', '--all-targets')
        for name in ([provider] if family == 'messaging' else [family, provider]):
            test = (repo / f'cli/tests/fixtures/rust_{"sms" if family == "messaging" else "billing"}_sdk_native_runtime.rs').read_text()
            if name == 'stripe':
                test = test.replace('use billing_sdk::*;', 'use stripe_sdk::*;')
                # Configured Stripe documents middleware authentication failures;
                # the base module's native route leaves the 401 undeclared.
                boundary = 'fn assert_missing_authorization('
                assert test.count(boundary) == 1
                test = test[:test.index(boundary)] + '''fn assert_missing_authorization(response: Result<GetPlanResponses, SdkError>) {
    assert!(matches!(response, Ok(GetPlanResponses::Status401(message))
        if message == "Authorization token required."));
}
'''
            if not (app / 'src' / name / 'sdk/rust/.forklaunch-sdk-local').is_file():
                test = test.replace('Client::new(', 'ForkLaunchClient::new(')
            manifests[name] = rust_consumer(app, root / (name + '-consumer'), name,
                                           package=name + '-sdk', test_source=test)
            run('cargo', 'test', '--manifest-path', manifests[name], '--test', 'runtime', '--no-run')
    for name, count in ([(family, 5), (provider, 6)] if family == 'messaging' else [(family, 33), (provider, 35)]):
        artifact = json.loads(contract_artifact(app, name).read_text())
        assert len(artifact['routes']) == count, (name, len(artifact['routes']))
    if not env.get('FORKLAUNCH_TEST_POSTGRES_URL'):
        run('docker', 'run', '--detach', '--name', container, '--publish', '127.0.0.1::5432',
            '--tmpfs', '/var/lib/postgresql/data:rw,size=256m', '--env', 'POSTGRES_PASSWORD=parity-test-only',
            '--env', 'POSTGRES_DB=parity', 'postgres:16-alpine')
        owned_database = True
        for _ in range(60):
            if subprocess.run(['docker', 'exec', container, 'pg_isready', '-U', 'postgres', '-d', 'parity'], capture_output=True).returncode == 0:
                break
            time.sleep(1)
        else:
            raise AssertionError('PostgreSQL did not start')
        port = subprocess.check_output(['docker', 'port', container, '5432/tcp'], text=True).strip()
        env['FORKLAUNCH_TEST_POSTGRES_URL'] = f'postgres://postgres:parity-test-only@{port}/parity'
    run('cargo', 'run', '-p', family, '--', '--migrate', extra={'DATABASE_URL': env['FORKLAUNCH_TEST_POSTGRES_URL']})
    seed = app / 'src' / family / 'examples/seed.rs'
    seed.parent.mkdir(exist_ok=True)
    shutil.copy2(repo / f'cli/tests/fixtures/rust_{"sms" if family == "messaging" else "billing"}_seed.rs', seed)
    run('cargo', 'run', '-p', family, '--example', 'seed')
    role_created = True
    for name, manifest in manifests.items():
        key = 'SMS' if family == 'messaging' else name.upper()
        env[f'FORKLAUNCH_TEST_{key}_SDK_MANIFEST'] = str(manifest)
    target = Path(env['CARGO_TARGET_DIR']) / 'debug'
    run('node', repo / f'cli/tests/rust_{family}_runtime.mjs', target / family, target / provider, entry)
    run('cargo', 'run', '-p', family, '--example', 'seed', '--', '--inspect')
    renamed = 'texting' if family == 'messaging' else 'payments'
    command('change', 'service', '-N', renamed, '--confirm', path=source)
    assert not source.exists()
    source = app / 'src' / renamed
    assert marker in (source / 'lib.rs').read_text()
    assert 'marker = "preserved"' in (source / 'Cargo.toml').read_text()
    command('sync', 'service', renamed)
    command('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    run('cargo', 'check', '--workspace', '--all-targets')
    command('delete', 'service', renamed, '--continue')
    assert not source.exists()
    assert not (app / f'src/client-sdk/rust/{renamed}.rs').exists()
    assert not (app / f'.forklaunch/contracts/{renamed}').exists()
    receipt = os.environ.get('FORKLAUNCH_PROVIDER_MODULE_RECEIPT')
    if receipt:
        Path(receipt).write_text(json.dumps({'schemaVersion': 1, 'cliSha256': digest, 'module': family,
            'passed': True, 'checks': ['raw neutral init', 'relocation and source preservation', 'both SDK imports',
                'public TypeScript runtime', 'typed Rust module consumers', 'real PostgreSQL', 'provider wire protocols',
                'encryption, authorization and compliance', 'HTTP/MCP', 'rename/sync/delete']}, indent=2) + '\n')
    passed = True
finally:
    if role_created:
        subprocess.run(['cargo', 'run', '-p', family, '--example', 'seed', '--', '--cleanup'], cwd=app, env=env, timeout=180)
    if owned_database:
        subprocess.run(['docker', 'rm', '-f', container], capture_output=True, timeout=60)
    if passed and not os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
