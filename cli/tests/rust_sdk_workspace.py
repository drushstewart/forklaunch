#!/usr/bin/env python3
"""Build real TypeScript-first mixed workspaces with PNPM and Bun after SDK generation."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
socket = os.environ.get('FORKLAUNCH_TEST_SOCKET_WORKSPACE') == '1'
output = repo / 'cli/tests/output' / ('rust-socket-workspace' if socket else 'rust-sdk-workspace')
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'FORKLAUNCH_ACCOUNT': 'parity-offline-sdk-workspace', 'CARGO_INCREMENTAL': '0'}
for runtime in os.environ.get('FORKLAUNCH_TEST_WORKSPACE_RUNTIMES', 'node,bun').split(','):
    assert runtime in ['node', 'bun']
    app = root / runtime
    def cli(*args):
        subprocess.run([str(binary), *args, '--path', str(app)], cwd=root, env=env, check=True, timeout=180)
    cli('init', 'application', runtime + '-workspace', '--language', 'typescript', '--modules-path', 'modules',
        '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'eslint',
        '--http-framework', 'express', '--runtime', runtime, '--test-framework', 'vitest',
        '-D', 'SDK workspace fixture', '-A', 'ForkLaunch', '-L', 'MIT')
    cli('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API')
    cli('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
    before = {str(p.relative_to(app)): p.read_bytes() for p in (app / 'modules').rglob('*') if p.is_file()}
    (app / 'package.json').write_text(json.dumps({'private': True, 'scripts': {'owned': 'echo application-owned'}}))
    (app / 'pnpm-workspace.yaml').write_text("packages: ['tools/*']\nminimumReleaseAge: 1440\n")
    if socket:
        artifact = app / '.forklaunch/socket-contracts/native-api/socket-contract.json'
        artifact.parent.mkdir(parents=True, exist_ok=True)
        artifact.write_bytes((repo / 'cli/tests/fixtures/rust_socket_contract.json').read_bytes())
        cli('sdk', 'generate', '--target', 'typescript', '--from', 'socket-contract', '--service', 'native-api')
        cli('sdk', 'generate', '--target', 'typescript', '--from', 'socket-contract', '--service', 'native-api', '--check')
        workspace = app / 'pnpm-workspace.yaml'
        workspace.write_text(''.join(line for line in workspace.read_text().splitlines(keepends=True) if 'sdks/typescript-websocket/' not in line))
        stale = workspace.read_bytes()
        package_before_check = (app / 'package.json').read_bytes()
        checked = subprocess.run([str(binary), 'sdk', 'generate', '--target', 'typescript', '--from', 'socket-contract', '--service', 'native-api', '--check', '--path', str(app)], cwd=root, env=env, capture_output=True, text=True, timeout=180)
        assert checked.returncode != 0 and 'out of date' in checked.stderr + checked.stdout
        assert workspace.read_bytes() == stale and (app / 'package.json').read_bytes() == package_before_check
        cli('sdk', 'generate', '--target', 'typescript', '--from', 'socket-contract', '--service', 'native-api')
    else:
        artifact = app / '.forklaunch/openapi/legacy-api/openapi.json'
        artifact.parent.mkdir(parents=True, exist_ok=True)
        artifact.write_text(json.dumps({'openapi': '3.1.0', 'info': {'title': 'Owned workspace fixture', 'version': '1'}, 'paths': {}}))
        cli('sdk', 'generate', '--target', 'typescript', '--from', 'openapi', '--service', 'legacy-api')
    assert before == {str(p.relative_to(app)): p.read_bytes() for p in (app / 'modules').rglob('*') if p.is_file()}
    if runtime == 'bun':
        policy_path = app / 'bunfig.toml'
        policy = tomllib.loads(policy_path.read_text())
        original_policy = tomllib.loads((app / 'modules/bunfig.toml').read_text())
        assert policy['install']['minimumReleaseAge'] == 86400
        assert policy['install']['minimumReleaseAgeExcludes'] == original_policy['install']['minimumReleaseAgeExcludes']
        policy_path.write_text('# application policy\n' + policy_path.read_text().replace('minimumReleaseAge = 86400', 'minimumReleaseAge = 172800'))
        cli('sdk', 'generate', '--target', 'typescript', '--from', 'socket-contract' if socket else 'openapi', '--service', 'native-api' if socket else 'legacy-api')
        assert policy_path.read_text().startswith('# application policy\n')
        assert tomllib.loads(policy_path.read_text())['install']['minimumReleaseAge'] == 172800
    package = json.loads((app / 'package.json').read_text())
    assert package['scripts']['owned'] == 'echo application-owned'
    assert 'typescript' in package['devDependencies'] and 'tsx' in package['devDependencies']
    manager = 'bun' if runtime == 'bun' else 'pnpm'
    subprocess.run([manager, 'install', '--ignore-scripts'], cwd=app, env=env, check=True, timeout=300)
    if runtime == 'node':
        listed = json.loads(subprocess.check_output(['pnpm', 'list', '--recursive', '--depth', '-1', '--json'], cwd=app, env=env))
        names = {p.get('name') for p in listed}
        assert {f'@{runtime}-workspace/{name}' for name in ['core', 'monitoring', 'legacy-api', 'client-sdk']} <= names
        assert ('native-api-websocket-sdk' if socket else '@forklaunch-generated/sdk') in names
    else:
        assert {'modules/*', 'sdks/typescript-websocket/*' if socket else 'sdks/typescript'} <= set(package['workspaces'])
    build = subprocess.run([manager, 'run', 'build'], cwd=app, env=env, capture_output=True, text=True, timeout=1800)
    print(build.stdout, flush=True)
    print(build.stderr, flush=True)
    build.check_returncode()
    if runtime == 'bun':
        for name in ['core', 'monitoring', 'legacy-api', 'client-sdk']:
            assert (build.stdout + build.stderr).count(f'@bun-workspace/{name} build: Exited with code 0') == 1, f'{name} must build exactly once'
    for name in ['core', 'monitoring', 'legacy-api', 'client-sdk']:
        assert any((app / 'modules' / name).rglob('*.tsbuildinfo')), f'{name} was omitted from root build'
    subprocess.run(['node', str(repo / 'cli/tests/rust_typescript_service_startup.mjs'), str(app)], cwd=app, env=env, check=True, timeout=120)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=app, env=env))
    subprocess.run(['node', str(repo / 'cli/tests/rust_mcp_service_startup.mjs'), str(Path(metadata['target_directory']) / 'debug/native-api')], cwd=app, env=env, check=True, timeout=120)
    print(f'PASS {runtime} root workspace installation/build includes TypeScript modules and SDK; both languages answer HTTP/MCP', flush=True)
print(f'SDK workspace fixture evidence: {root}', flush=True)
