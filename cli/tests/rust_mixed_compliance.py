#!/usr/bin/env python3
"""Compile real mixed scaffolds and fan out through real SDKs to owned HTTP fixtures."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-mixed-compliance'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
app = root / 'app'
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'FORKLAUNCH_ACCOUNT': 'parity-offline-compliance'}


def cli(*args):
    subprocess.run([str(binary), *args, '--path', str(app)], cwd=root, env=env, check=True, timeout=180)


for variant in ['iam-base', 'iam-better-auth']:
    app = root / variant
    cli('init', 'application', 'mixed-compliance', '--language', 'rust', '-D', 'Mixed compliance', '-A', 'ForkLaunch', '-L', 'MIT')
    cli('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
    cli('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API before IAM')
    cli('init', 'module', 'iam', '--language', 'typescript', '-m', variant, '-d', 'postgresql')
    # An owned native OpenAPI profile exercises the real generated HTTP SDK. HTTP
    # responders validate fan-out transport; persistence behavior has separate suites.
    schemas = {
        'erase': {'type': 'object', 'properties': {'recordsDeleted': {'type': 'integer'}, 'entitiesAffected': {'type': 'array', 'items': {'type': 'string'}}}, 'required': ['recordsDeleted', 'entitiesAffected']},
        'export': {'type': 'object', 'properties': {'userId': {'type': 'string'}, 'entities': {'type': 'object', 'additionalProperties': {'type': 'array', 'items': {'type': 'string'}}}}, 'required': ['userId', 'entities']},
    }
    paths = {}
    for action, method in [('erase', 'delete'), ('export', 'get')]:
        responses = {'200': {'description': 'OK', 'content': {'application/json': {'schema': schemas[action]}}}}
        if action == 'erase':
            responses['409'] = {'description': 'Conflict', 'content': {'application/json': {'schema': {'type': 'object', 'properties': {'conflict': {'type': 'string'}}, 'required': ['conflict']}}}}
        paths[f'/compliance/{action}/{{userId}}'] = {method: {'operationId': action + 'UserData', 'parameters': [{'in': 'path', 'name': 'userId', 'required': True, 'schema': {'type': 'string'}}], 'responses': responses}}
    spec = {'openapi': '3.1.0', 'info': {'title': 'Native compliance fixture', 'version': '1'}, 'paths': paths}
    artifact = app / '.forklaunch/openapi/native-api/openapi.json'
    artifact.parent.mkdir(parents=True, exist_ok=True)
    artifact.write_text(json.dumps(spec))
    cli('sdk', 'generate', '--target', 'typescript', '--from', 'openapi', '--service', 'native-api')
    client = app / 'modules/client-sdk'
    source = (client / 'compliance.ts').read_text()
    assert 'nativeApiSdkClient' not in source
    assert 'nativeApi: ComplianceClient<' in source
    assert 'legacyApi: RequireCompliance<' in source
    subprocess.run(['pnpm', 'install', '--ignore-scripts', '--no-frozen-lockfile'], cwd=app, env=env, check=True, timeout=300)
    subprocess.run(['pnpm', '-r', '--no-bail', 'run', 'build'], cwd=app, env=env, check=True, timeout=300)
    (client / 'mixedCompliance.fixture.ts').write_text((repo / 'cli/tests/fixtures/rust_mixed_compliance.ts').read_text())
    (client / 'tsconfig.compliance-fixture.json').write_text(json.dumps({'extends': '../tsconfig.base.json', 'compilerOptions': {'noEmit': True, 'types': ['node']}, 'files': ['mixedCompliance.fixture.ts'], 'include': []}))
    subprocess.run([str(app / 'node_modules/.bin/tsc'), '-p', 'tsconfig.compliance-fixture.json'], cwd=client, env=env, check=True, timeout=120)
    subprocess.run([str(app / 'node_modules/.bin/tsx'), 'mixedCompliance.fixture.ts'], cwd=client, env=env, check=True, timeout=120)
print(f'Mixed compliance fixture evidence: {root}', flush=True)
