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
env = {**os.environ, 'FORKLAUNCH_ACCOUNT': 'parity-offline-dev', 'CARGO_INCREMENTAL': '0'}
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

try:
    process = subprocess.Popen([str(binary), 'dev', 'legacy-api', '--path', str(app), '--project-name', project,
        '--compose-file', str(app / 'dev.fixture.yaml'), '--no-deps', '--sdk-target', 'typescript'],
        cwd=app, env=env, stdout=log, stderr=subprocess.STDOUT)
    message = wait(http)
    shared = app / 'modules/core/development.schema.ts'
    assert 'sharedLibraryMarker' in shared.read_text()
    shared.write_text(shared.read_text().replace('sharedLibraryMarker', 'sharedLibraryReplayMarker'))
    sdk = app / 'sdks/typescript/src/index.ts'
    wait(lambda: 'sharedLibraryReplayMarker' in sdk.read_text(), seconds=120)
    assert wait(http) == message
    subprocess.run(['node', str(repo / 'cli/tests/rust_mixed_dev_probe.mjs'), '-', endpoint(port), endpoint(port + 2000) + '/mcp', message], check=True, timeout=90)
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=90) == 0
    commands = subprocess.check_output(['ps', '-axo', 'command'], text=True)
    assert f'sdk watch --path {app} --target typescript' not in commands
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
    if containers:
        subprocess.run(['docker', 'rm', '-f', '-v', *containers], check=True)
    for resource in ['volume', 'network']:
        resources = subprocess.check_output(['docker', resource, 'ls', '--filter', f'label=com.docker.compose.project={project}', '--format', '{{.ID}}' if resource == 'network' else '{{.Name}}'], text=True).split()
        if resources:
            subprocess.run(['docker', resource, 'rm', *resources], check=True)
    subprocess.run(['docker', 'image', 'rm', config['services']['legacy-api']['image']], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
