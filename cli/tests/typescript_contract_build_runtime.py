#!/usr/bin/env python3
"""Prove explicit TS contract builds emit both SDKs without bootstrapping a server."""
import json
import os
from pathlib import Path
import shutil
import shlex
import sys
import subprocess
import tempfile
import time

REPO = Path(__file__).resolve().parents[2]
CLI = Path(os.environ.get('FORKLAUNCH_CLI', REPO / 'cli/target/debug/forklaunch'))
ROOT = Path(tempfile.mkdtemp(prefix='forklaunch-contract-build-'))
APP = ROOT / 'app'

def run(*args, cwd=ROOT):
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True, env={**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1'})
    if result.returncode:
        raise RuntimeError(f'{args[:3]} failed:\n{result.stdout[-7000:]}\n{result.stderr[-7000:]}')
    return result.stdout

def cli(*args):
    return run(str(CLI), *args, '--path', str(APP))

def sdks(*names):
    # SDKs are built by the CLI; `pnpm build` is plain tsc.
    for name in names:
        cli('sdk', 'build', '--skip-compile', '--service', name)

try:
    cli('init', 'application', 'contract-proof', '--language', 'typescript', '--modules-path', 'modules',
        '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'eslint',
        '--http-framework', 'express', '--runtime', 'node', '--test-framework', 'vitest',
        '-D', 'Contract proof', '-A', 'ForkLaunch', '-L', 'MIT')
    # Contract builds exist for Rust consumers; a TypeScript-only app keeps tsc -b.
    cli('init', 'service', 'consumer', '--language', 'rust', '-D', 'Rust consumer')
    cli('init', 'service', 'notes', '--language', 'typescript', '-d', 'postgresql', '-D', 'Notes')
    service = APP / 'modules/notes'
    assert (service / 'api/contracts/notes.contract.ts').exists()
    for file in ['bootstrapper.ts', 'server.ts']:
        with (service / file).open('a') as output:
            output.write('\nthrow new Error("BOOTSTRAP_MUST_NOT_RUN");\n')
    run('pnpm', 'install', '--ignore-scripts', cwd=APP / 'modules')
    assert json.loads((service / 'package.json').read_text())['scripts']['build'] == 'tsc -b'
    assert not (service / 'sdk/contract.json').exists()
    environment = {**os.environ, 'CARGO_TARGET_DIR': str(REPO / 'cli/target'), 'CARGO_INCREMENTAL': '0', 'RUSTUP_TOOLCHAIN': 'stable'}
    result = subprocess.run([sys.executable, str(REPO / 'cli/tests/fixtures/worker_runtime_lock.py'), '-c',
        shlex.join(['pnpm', '--dir', str(APP / 'modules'), 'build'])], cwd=REPO, env=environment, text=True, capture_output=True)
    assert result.returncode == 0, result.stdout[-4000:] + result.stderr[-6000:]
    sdks('notes')
    assert (service / 'sdk/contract.json').is_file()
    print('PASS fresh default normal TypeScript build emits contract without bootstrap evaluation', flush=True)
    cli('sdk', 'build', '--service', 'notes')
    contract_file = service / 'sdk/contract.json'
    contract = json.loads(contract_file.read_text())
    assert contract['sdkCompiler'] == 'forklaunch.sdk.http.v1'
    assert contract['rustClientName'] == 'Client'
    assert {route['method'] for route in contract['routes']} == {'GET', 'POST'}
    assert all(route['path'] == '/notes' for route in contract['routes'])
    schema = contract['routes'][0]['responses'][0]['body']['schema']['jsonSchema']
    assert schema['properties']['message']['type'] in ('string', ['string']), schema
    ts_outputs = [service / 'sdk.ts']
    authored_sdk = (service / 'sdk.ts').read_bytes()
    assert not (APP / 'sdks/typescript/notes').exists()
    rust_outputs = list((service / 'sdk/rust').rglob('*.rs'))
    assert 'SdkClient' in (service / 'sdk.ts').read_text()
    assert any('notes_notes_get' in file.read_text() for file in rust_outputs)
    before = {file: file.stat().st_mtime_ns for file in [contract_file, *ts_outputs, *rust_outputs]}
    cli('sdk', 'build', '--service', 'notes')
    assert before == {file: file.stat().st_mtime_ns for file in before}
    print('PASS isolated export, both canonical SDKs, stable output timestamps', flush=True)
    shared = service / 'api/contracts/notes.contract.ts'
    original = shared.read_text()
    shared.write_text("import '../../bootstrapper';\n" + original)
    rejected = subprocess.run([str(CLI), 'sdk', 'build', '--skip-compile', '--service', 'notes', '--path', str(APP)], text=True, capture_output=True)
    assert rejected.returncode != 0
    assert 'cannot import runtime infrastructure' in rejected.stderr, rejected.stderr
    assert before == {file: file.stat().st_mtime_ns for file in before}
    shared.write_text(original)
    schema_file = service / 'domain/schemas/notes.schema.ts'
    schema_original = schema_file.read_text()
    watch_log = ROOT / 'watch.log'
    with watch_log.open('w') as output:
        watcher = subprocess.Popen([str(CLI), 'sdk', 'build', '--watch', '--service', 'notes', '--path', str(APP)], stdout=output, stderr=subprocess.STDOUT)
        try:
            time.sleep(3)
            schema_file.write_text(schema_original.replace('message: string', 'message: string.min(3)'))
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                if 'minLength' in contract_file.read_text() and any('minLength' in file.read_text() for file in rust_outputs):
                    break
                if watcher.poll() is not None:
                    raise AssertionError(watch_log.read_text())
                time.sleep(.3)
            else:
                raise AssertionError('Watch did not regenerate both languages: ' + watch_log.read_text())
        finally:
            watcher.terminate()
            watcher.wait(timeout=15)
            schema_file.write_text(schema_original)
    print('PASS runtime-import rejection before evaluation and watch regenerates both SDK languages', flush=True)
    workspace_manager = json.loads((APP / 'modules/package.json').read_text())['packageManager']
    cli('sdk', 'build', '--install-hooks', '--service', 'notes')
    assert json.loads((APP / 'modules/package.json').read_text())['packageManager'] == workspace_manager
    assert not (APP / 'package.json').exists()
    run('pnpm', 'install', '--ignore-scripts', cwd=APP / 'modules')
    assert (service / 'sdk.ts').read_bytes() == authored_sdk
    # Serialize source-snapshot compilation with other shared Cargo fixtures.
    environment = {**os.environ, 'CARGO_TARGET_DIR': str(REPO / 'cli/target'), 'CARGO_INCREMENTAL': '0', 'RUSTUP_TOOLCHAIN': 'stable'}
    result = subprocess.run([sys.executable, str(REPO / 'cli/tests/fixtures/worker_runtime_lock.py'), '-c',
        shlex.join(['pnpm', '--dir', str(service), 'build'])], cwd=REPO, env=environment, text=True, capture_output=True)
    assert result.returncode == 0, result.stdout[-4000:] + result.stderr[-6000:]
    sdks('notes')
    assert sorted(p.name for p in (APP / '.forklaunch').iterdir()) == ['manifest.toml']
    print('PASS normal build plus CLI SDK build, no in-app compiler, generated SDK package typecheck', flush=True)
    cli('init', 'worker', 'jobs', '--language', 'typescript', '-t', 'database', '-d', 'postgresql', '-D', 'Default worker')
    assert json.loads((APP / 'modules/jobs/package.json').read_text())['scripts']['build'] == 'tsc -b'
    assert not (APP / 'modules/jobs/sdk/contract.json').exists()
    run('pnpm', 'install', '--ignore-scripts', cwd=APP / 'modules')
    result = subprocess.run([sys.executable, str(REPO / 'cli/tests/fixtures/worker_runtime_lock.py'), '-c',
        shlex.join(['pnpm', '--dir', str(APP / 'modules'), 'build'])], cwd=REPO, env=environment, text=True, capture_output=True)
    assert result.returncode == 0, result.stdout[-4000:] + result.stderr[-6000:]
    sdks('notes', 'jobs')
    for name in ['notes', 'jobs']:
        config = {'contract': f'modules/{name}/sdk/contract.json', 'rust': f'modules/{name}/sdk/rust/lib.rs',
                  'service': f'modules/{name}', 'typescript': f'modules/{name}/sdk/index.ts', 'typescriptOwned': True}
        assert len(json.loads((APP / config['contract']).read_text())['routes']) == 2
        assert config['typescriptOwned'] is True
        assert (APP / config['rust']).is_file()
        assert (APP / config['service'] / 'sdk.ts').is_file()
        assert not (APP / config['typescript']).exists()
    print('PASS new default worker build emits both SDKs and preserves service assets', flush=True)

    # A source-only transport edit must not require reinstalling compiler hooks
    # or rewriting the consumer's dependency manifest.
    manifests = {path: path.read_bytes() for path in (APP / 'modules').glob('*/sdk/rust/Cargo.toml')}
    assert manifests
    shared.write_text(original.replace('body: NotesRequestSchema,',
        "body: { contentType: 'multipart/form-data', multipartForm: NotesRequestSchema },"))
    test_file = service / '__test__/notes.test.ts'
    test_source = test_file.read_text()
    for value in ['mockNotesData', 'invalidData']:
        test_source = test_source.replace(f'body: {value}',
            f"body: {{ contentType: 'multipart/form-data', multipartForm: {value} }}")
    test_file.write_text(test_source)
    result = subprocess.run([sys.executable, str(REPO / 'cli/tests/fixtures/worker_runtime_lock.py'), '-c',
        shlex.join(['pnpm', '--dir', str(service), 'build'])], cwd=REPO, env=environment, text=True, capture_output=True)
    assert result.returncode == 0, result.stdout[-4000:] + result.stderr[-6000:]
    sdks('notes', 'jobs')
    updated = json.loads(contract_file.read_text())
    post = next(route for route in updated['routes'] if route['method'] == 'POST')
    assert post['request']['body']['kind'] == 'multipart'
    assert manifests == {path: path.read_bytes() for path in manifests}
    assert (service / 'sdk.ts').read_bytes() == authored_sdk
    for manifest in manifests:
        result = subprocess.run([sys.executable, str(REPO / 'cli/tests/fixtures/worker_runtime_lock.py'), '-c',
            shlex.join(['cargo', '+stable', 'check', '--manifest-path', str(manifest)])],
            cwd=REPO, env=environment, text=True, capture_output=True)
        assert result.returncode == 0, result.stdout[-4000:] + result.stderr[-6000:]
    print('PASS JSON to multipart normal build updates both SDKs without dependency or hook reinstall', flush=True)



finally:
    if os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):
        print('Fixture retained:', ROOT, flush=True)
    else:
        shutil.rmtree(ROOT)
