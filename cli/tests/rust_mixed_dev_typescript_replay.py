#!/usr/bin/env python3
"""Replay a completed owned mixed fixture with only its TypeScript service selected."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import urllib.request
import uuid

repo = Path(__file__).resolve().parents[2]
app = Path(sys.argv[1]).resolve()
assert app.is_relative_to(repo / 'cli/tests/output/rust-mixed-dev')
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
project = 'typescript-replay-' + uuid.uuid4().hex[:12]
env = {**os.environ, 'FORKLAUNCH_FRAMEWORK_SNAPSHOT': '1', 'FORKLAUNCH_ACCOUNT': 'parity-offline-dev', 'CARGO_INCREMENTAL': '0'}
config = json.loads(subprocess.check_output(['docker', 'compose', '--project-directory', str(app), '-f', str(app / 'docker-compose.yaml'), 'config', '--format', 'json'], text=True))
port = int(config['services']['legacy-api']['environment']['PORT'])
log_path = app.parent / 'typescript-replay.log'
log = log_path.open('w')
process = None

def ids():
    return subprocess.check_output(['docker', 'ps', '-a', '--filter', f'label=com.docker.compose.project={project}', '--format', '{{.ID}}'], text=True).split()

def endpoint(number):
    containers = ids()
    for container in json.loads(subprocess.check_output(['docker', 'inspect', *containers])) if containers else []:
        if container['Config']['Labels']['com.docker.compose.service'] == 'legacy-api':
            bindings = container['NetworkSettings']['Ports'].get(f'{number}/tcp')
            if bindings:
                return 'http://127.0.0.1:' + bindings[0]['HostPort']
    raise KeyError('No active service port')

def wait(check, seconds=240):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        assert process.poll() is None, log_path.read_text()[-12000:]
        try:
            result = check()
            if result:
                return result
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(0.2)
    raise AssertionError(log_path.read_text()[-12000:])

def http():
    with urllib.request.urlopen(endpoint(port) + '/legacy-api', timeout=2) as response:
        return json.load(response)['message']

def started_at():
    containers = ids()
    for container in json.loads(subprocess.check_output(['docker', 'inspect', *containers])) if containers else []:
        if container['Config']['Labels']['com.docker.compose.service'] == 'legacy-api' and container['State']['Running']:
            return container['State']['StartedAt']
    raise KeyError('No running service')

def probe_surfaces(message):
    deadline = time.monotonic() + 180
    observed = None
    stable_since = time.monotonic()
    last_failure = ''
    while time.monotonic() < deadline:
        assert process.poll() is None, log_path.read_text()[-12000:]
        try:
            current = started_at()
            if current != observed:
                observed, stable_since = current, time.monotonic()
            if time.monotonic() - stable_since < 3:
                time.sleep(0.5)
                continue
            assert http() == message
            result = subprocess.run(['node', str(repo / 'cli/tests/rust_mixed_dev_probe.mjs'), '-', endpoint(port), endpoint(port + 2000) + '/mcp', message],
                capture_output=True, text=True, timeout=90)
        except (OSError, ValueError, KeyError):
            time.sleep(0.5)
            continue
        if result.returncode == 0:
            print(result.stdout, end='', flush=True)
            return
        last_failure = result.stdout + result.stderr
        if not any(code in last_failure for code in ['ECONNREFUSED', 'ECONNRESET', 'UND_ERR_SOCKET']):
            raise AssertionError(last_failure)
        stable_since = time.monotonic()
        time.sleep(0.5)
    raise AssertionError(f'HTTP/MCP endpoints did not settle: {last_failure}\n{log_path.read_text()[-12000:]}')

try:
    process = subprocess.Popen([str(binary), 'dev', 'legacy-api', '--path', str(app), '--project-name', project,
        '--compose-file', str(app / 'dev.fixture.yaml'), '--no-deps', '--sdk-target', 'all',
        *(['--no-build'] if '--reuse-image' in sys.argv[2:] else [])],
        cwd=app, env=env, stdout=log, stderr=subprocess.STDOUT)
    message = wait(http, seconds=1200)
    before_edit = started_at()
    shared = app / 'src/core/development.schema.ts'
    assert 'sharedLibraryMarker' in shared.read_text()
    shared.write_text(shared.read_text().replace('sharedLibraryMarker', 'sharedLibraryReplayMarker'))
    sdk = app / 'src/legacy-api/sdk/rust'
    artifact = app / 'src/legacy-api/sdk/openapi.json'
    wait(lambda: any('shared_library_replay_marker' in path.read_text() for path in sdk.glob('*.rs')) and 'sharedLibraryReplayMarker' in artifact.read_text(), seconds=120)
    # Bun can hot-reload before Compose completes its own restart. Require that
    # restart before capturing ephemeral host ports for the final HTTP/MCP probe.
    wait(lambda: started_at() != before_edit)
    assert wait(http) == message
    probe_surfaces(message)
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=90) == 0
    commands = subprocess.check_output(['ps', '-axo', 'command'], text=True)
    assert f'sdk watch --path {app} --target all' not in commands
    print(f'PASS TypeScript-only selection, shared-library rebuild, combined SDK refresh, HTTP/MCP and owned watcher shutdown: {app}', flush=True)
finally:
    if process is not None and process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=60)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    log.close()
    containers = ids()
    images = {container['Config']['Image'] for container in json.loads(subprocess.check_output(['docker', 'inspect', *containers]))} if containers else set()
    if containers:
        subprocess.run(['docker', 'rm', '-f', '-v', *containers], check=True)
    for resource in ['volume', 'network']:
        resources = subprocess.check_output(['docker', resource, 'ls', '--filter', f'label=com.docker.compose.project={project}', '--format', '{{.ID}}' if resource == 'network' else '{{.Name}}'], text=True).split()
        if resources:
            subprocess.run(['docker', resource, 'rm', *resources], check=True)
    for image in images | {config['services']['legacy-api']['image']}:
        subprocess.run(['docker', 'image', 'rm', image], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
