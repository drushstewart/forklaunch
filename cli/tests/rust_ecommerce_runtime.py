#!/usr/bin/env python3
"""Raw ecommerce scaffolding, real provider/storage boundaries and CLI lifecycle."""
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
    raw_application, build_public_runtime, rust_consumer, typecheck, PostgresDatabase,
    sdk_type_inputs, contract_artifact,
)

repo = Path(__file__).resolve().parents[2]
fixtures = repo / 'cli/tests/fixtures'
root = Path(tempfile.mkdtemp(prefix='forklaunch-ecommerce-')).resolve()
app = root / 'original'
env = dict(os.environ)
cli = root / 'forklaunch'
shutil.copy2(env['FORKLAUNCH_CLI'], cli)
digest = hashlib.sha256(cli.read_bytes()).hexdigest()
print('CLI SHA256:', digest, flush=True)
env['FORKLAUNCH_TEST_ROLE'] = f'fl_ecom_{int(time.time())}_{os.getpid()}'
database = PostgresDatabase(env, 'forklaunch-ecommerce')
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


def snapshot():
    return {str(p.relative_to(app)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in app.rglob('*') if p.is_file()}


try:
    raw_application(lambda *args: run(*args, cwd=root), cli, root / 'dry', 'dry', dryrun=True)
    assert not (root / 'dry').exists()
    raw_application(lambda *args: run(*args, cwd=root), cli, app, 'shop-app')
    for name in ['ecommerce', 'spare-shop']:
        command('init', 'module', name, '--language', 'rust', '-m', 'ecommerce-stripe')
    command('init', 'service', 'runtime-host', '--language', 'rust', '-D', 'SDK runtime fixture')
    app.rename(root / 'relocated')
    app = root / 'relocated'
    run('python3', fixtures / 'local_crates_patch.py', app, repo)
    run('python3', fixtures / 'rust_ecommerce_compose.py', 'prepare')
    source = app / 'src/spare-shop'
    with (source / 'lib.rs').open('a') as file:
        file.write('\npub const USER_SHOP_MARKER: &str = "preserved";\n')
    with (source / 'Cargo.toml').open('a') as file:
        file.write('\n[package.metadata.user]\nmarker = "preserved"\n')
    command('sync', 'service', 'spare-shop')
    command('init', 'module', 'spare-shop', '--language', 'rust', '-m', 'ecommerce-stripe')
    run('python3', fixtures / 'rust_ecommerce_compose.py', 'preserved')
    assert 'USER_SHOP_MARKER' in (source / 'lib.rs').read_text()
    assert all(key in (app / 'docker-compose.yaml').read_text() for key in ['ECOMMERCE_DATABASE_URL', 'STRIPE_API_KEY'])
    before = snapshot()
    command('init', 'service', 'spare-shop', '--language', 'rust', '-d', 'postgresql', '-D', 'Invalid', success=False)
    assert snapshot() == before
    run('cargo', 'build', '--workspace')
    # Blueprint crates are published; their tests run in the blueprint workspace.
    blueprint = repo / 'blueprint-rs/Cargo.toml'
    run('cargo', 'test', '--manifest-path', blueprint, '-p', 'forklaunch-ecommerce-stripe', '--test', 'provider')
    database.start()
    run('cargo', 'test', '--manifest-path', blueprint, '-p', 'forklaunch-ecommerce-base', '--all-features', '--test', 'reconciliation', '--', '--include-ignored')
    run(cli, 'openapi', 'export', '--path', app,
        extra={'OTEL_EXPORTER_OTLP_ENDPOINT': 'invalid', 'OTEL_EXPORTER_OTLP_PROTOCOL': 'invalid'})
    runtime = root / 'public-runtime'
    entry = build_public_runtime(run, app, runtime, repo)
    env['TSC_BIN'] = str(runtime / 'node_modules/.bin/tsc')
    for origin in ['contract', 'openapi']:
        command('sdk', 'generate', '--target', 'all', '--from', origin)
        typecheck(run, app, runtime, sdk_type_inputs(app, ['ecommerce', 'spare-shop']))
        run('cargo', 'check', '--workspace', '--all-targets')
        test = (fixtures / 'rust_ecommerce_sdk_native_runtime.rs').read_text()
        if not (app / 'src/ecommerce/sdk/rust/.forklaunch-sdk-local').is_file():
            test = test.replace('Client::new(', 'ForkLaunchClient::new(')
        consumer = rust_consumer(app, root / 'rust-consumer', 'ecommerce', package='ecommerce-sdk',
                                 test_source=test)
        run('cargo', 'test', '--manifest-path', consumer, '--test', 'runtime', '--no-run')
    for name in ['ecommerce', 'spare-shop']:
        artifact = json.loads(contract_artifact(app, name).read_text())
        assert len(artifact['routes']) == 33, (name, len(artifact['routes']))
    run('cargo', 'run', '-p', 'ecommerce', '--', '--migrate', extra={'DATABASE_URL': env['FORKLAUNCH_TEST_POSTGRES_URL']})
    examples = app / 'src/ecommerce/examples'
    examples.mkdir(exist_ok=True)
    for name in ['seed', 'webhook_bridge']:
        shutil.copy2(fixtures / f'rust_ecommerce_{name}.rs', examples / f'{name}.rs')
    run('cargo', 'build', '-p', 'ecommerce', '--example', 'webhook_bridge')
    role_created = True
    run('cargo', 'run', '-p', 'ecommerce', '--example', 'seed')
    env['FORKLAUNCH_TEST_ECOMMERCE_SDK_MANIFEST'] = str(consumer)
    target = Path(env['CARGO_TARGET_DIR']) / 'debug'
    run('node', repo / 'cli/tests/rust_ecommerce_runtime.mjs', target / 'ecommerce', entry, target / 'examples/webhook_bridge')
    run('cargo', 'run', '-p', 'ecommerce', '--example', 'seed', '--', '--inspect')
    command('change', 'service', '-N', 'renamed-shop', '--confirm', path=source)
    run('python3', fixtures / 'rust_ecommerce_compose.py', 'renamed')
    source = app / 'src/renamed-shop'
    assert 'USER_SHOP_MARKER' in (source / 'lib.rs').read_text()
    assert 'marker = "preserved"' in (source / 'Cargo.toml').read_text()
    command('sync', 'service', 'renamed-shop')
    command('sdk', 'generate', '--target', 'all', '--from', 'contract', '--check')
    run('cargo', 'check', '--workspace', '--all-targets')
    command('delete', 'service', 'renamed-shop', '--continue')
    run('python3', fixtures / 'rust_ecommerce_compose.py', 'deleted')
    assert not source.exists()
    assert not (app / 'src/client-sdk/rust/renamed-shop.rs').exists()
    if os.environ.get('FORKLAUNCH_ECOMMERCE_RECEIPT'):
        Path(env['FORKLAUNCH_ECOMMERCE_RECEIPT']).write_text(json.dumps({
            'schemaVersion': 1, 'cliSha256': digest, 'passed': True,
            'checks': ['raw neutral init', 'relocation', 'both SDKs', 'HTTP/MCP',
                       'restricted PostgreSQL', 'Stripe and PayPal wire protocols',
                       'encrypted addresses and audit', 'inventory delivery',
                       'TS controller comparisons', 'rename/sync/delete and worker ownership'],
        }, indent=2) + '\n')
    passed = True
finally:
    if role_created:
        subprocess.run(['cargo', 'run', '-p', 'ecommerce', '--example', 'seed', '--', '--cleanup'], cwd=app, env=env, timeout=180)
    database.close()
    if passed and not os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        shutil.rmtree(root)
    else:
        print('Fixture retained:', root, flush=True)
