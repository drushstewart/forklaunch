#!/usr/bin/env python3
"""Compile raw mixed scaffolds and verify typed authenticated compliance fan-out."""
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

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'FORKLAUNCH_ACCOUNT': 'parity-offline-compliance',
       'CARGO_INCREMENTAL': '0', 'RUSTUP_TOOLCHAIN': 'stable'}
env.setdefault('CARGO_TARGET_DIR', str(repo / 'cli/target'))
if not env.get('FORKLAUNCH_WORKER_RUNTIME_LOCKED'):
    sys.exit(subprocess.run([sys.executable, str(repo / 'cli/tests/fixtures/worker_runtime_lock.py'),
        '-c', shlex.join([sys.executable, str(Path(__file__).resolve())])], env=env).returncode)
root = Path(tempfile.mkdtemp(prefix='forklaunch-mixed-compliance-')).resolve()
app = root / 'app'
print('CLI SHA256:', hashlib.sha256(binary.read_bytes()).hexdigest(), flush=True)


def run(*args, cwd=None):
    print('Running:', shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), cwd=cwd or app, env=env, check=True, timeout=1800)


def cli(*args):
    run(binary, *args, '--path', app, cwd=root)


try:
    for variant in ['iam-base', 'iam-better-auth']:
        app = root / variant
        cli('init', 'application', 'mixed-compliance', '--database', 'postgresql', '--validator', 'zod',
            '--formatter', 'prettier', '--linter', 'eslint', '--http-framework', 'express', '--runtime', 'node',
            '--test-framework', 'vitest', '-D', 'Mixed compliance', '-A', 'ForkLaunch', '-L', 'MIT')
        cli('init', 'service', 'native-api', '--language', 'rust', '-d', 'postgresql', '-D', 'Native API')
        cli('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API before IAM')
        cli('init', 'module', 'iam', '--language', 'typescript', '-m', variant, '-d', 'postgresql')
        # Build the actual native service first. Its compilation publishes the
        # local SDK needed by the normal root TypeScript workspace build.
        run('cargo', 'build', '-p', 'native-api')
        cli('sdk', 'generate', '--target', 'typescript', '--from', 'contract', '--service', 'native-api')
        run('pnpm', 'install', '--ignore-scripts', '--no-frozen-lockfile')
        run('pnpm', 'run', 'build')

        # An authored OpenAPI profile exercises the supported import/compiler
        # boundary and real public transport. Owned HTTP responders validate
        # fan-out/auth/status behavior; PostgreSQL protection has its own gate.
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
        manifest = tomllib.loads((app / '.forklaunch/manifest.toml').read_text())
        project = next(project for project in manifest['projects'] if project['name'] == 'native-api')
        assert project['resources']['database'] == 'postgresql'
        assert project['metadata']['sdkLayout'] == 'project-local-v1'
        artifact = app / project['metadata']['openapiPath']
        artifact.write_text(json.dumps(spec))
        cli('sdk', 'generate', '--target', 'typescript', '--from', 'openapi', '--service', 'native-api')
        cli('sdk', 'generate', '--target', 'typescript', '--from', 'openapi', '--service', 'native-api', '--check')
        sdk = app / 'src/native-api/sdk/typescript'
        assert (sdk / 'index.ts').read_text().strip() == "export type * from './sdk.js';"
        assert 'export type NativeApiSdkClient' in (sdk / 'sdk.ts').read_text()
        client = app / 'src/client-sdk'
        source = (client / 'compliance.ts').read_text()
        assert 'nativeApiSdkClient' not in source
        assert 'nativeApi: ComplianceClient<' in source
        assert 'legacyApi: RequireCompliance<' in source
        if variant == 'iam-better-auth':
            assert 'config.iam.core.compliance' in source
        else:
            assert 'config.iam.compliance' in source
        (client / 'mixedCompliance.fixture.ts').write_text((repo / 'cli/tests/fixtures/rust_mixed_compliance.ts').read_text())
        (client / 'tsconfig.compliance-fixture.json').write_text(json.dumps({'extends': '../tsconfig.base.json', 'compilerOptions': {'noEmit': True, 'types': ['node']}, 'files': ['mixedCompliance.fixture.ts'], 'include': []}))
        run(app / 'node_modules/.bin/tsc', '-p', 'tsconfig.compliance-fixture.json', cwd=client)
        run(app / 'node_modules/.bin/tsx', 'mixedCompliance.fixture.ts', cwd=client)
        print('PASS current mixed compliance with ' + variant, flush=True)
    print('PASS both IAM variants, compiled mixed applications and authenticated typed fan-out', flush=True)
finally:
    if env.get('FORKLAUNCH_KEEP_FIXTURE'):
        print('Mixed compliance fixture evidence:', root, flush=True)
    else:
        shutil.rmtree(root)
