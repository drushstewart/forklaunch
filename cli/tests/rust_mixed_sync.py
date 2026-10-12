#!/usr/bin/env python3
"""App-wide native/TypeScript discovery and shared SDK export through the CLI."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

repo = Path(__file__).resolve().parents[2]
binary = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch')).resolve()
output = repo / 'cli/tests/output/rust-mixed-sync'
output.mkdir(parents=True, exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='run.', dir=output)).resolve()
app = root / 'app'
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1', 'CARGO_INCREMENTAL': '0'}


def run(*args):
    subprocess.run([str(binary), *args, '--path', str(app)], cwd=root, env=env, check=True, timeout=1800)


run('init', 'application', 'mixed-sync', '--language', 'typescript', '--modules-path', 'modules',
    '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'eslint',
    '--http-framework', 'express', '--runtime', 'node', '--test-framework', 'vitest',
    '-D', 'Mixed sync', '-A', 'ForkLaunch', '-L', 'MIT')
run('init', 'service', 'legacy-api', '--language', 'typescript', '-d', 'postgresql', '-D', 'Legacy API')
run('init', 'library', 'discovered-models', '--language', 'typescript', '-D', 'Discovered models')
run('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
run('init', 'library', 'native-models', '--language', 'rust', '-D', 'Native models')
manifest_path = app / '.forklaunch/manifest.toml'
# Remove only this library's registration while leaving the actual TS scaffold.
text = manifest_path.read_text()
starts = [match.start() for match in re.finditer(r'^\[\[projects\]\]$', text, flags=re.M)]
for start, end in zip(starts, starts[1:] + [len(text)]):
    block = text[start:end]
    if '\nname = "discovered-models"\n' in block:
        other_table = re.search(r'^\[(?!\[?projects[.\]])', block, flags=re.M)
        stop = start + other_table.start() if other_table else end
        text = text[:start] + text[stop:]
        break
else:
    raise AssertionError('Missing library registration')
assert 'discovered-models' not in {p['name'] for p in tomllib.loads(text)['projects']}
# A registered native library is outside crates/ and needs its Cargo member restored.
(app / 'shared').mkdir()
(app / 'crates/native-models').rename(app / 'shared/native-models')
text = text.replace('crates/native-models', 'shared/native-models')
manifest_path.write_text(text)
cargo = app / 'Cargo.toml'
cargo.write_text(re.sub(r',\s*"crates/native-models"', '', cargo.read_text()))
source = app / 'shared/native-models/src/lib.rs'
source.write_text(source.read_text() + '\npub const USER_OWNED: bool = true;\n')
original = source.read_bytes()
# Use a real tsx export process with an owned export hook; no framework/DB startup
# is required just to prove language dispatch and artifact refresh.
tools = root / 'tools'
tools.mkdir()
(tools / 'package.json').write_text('{"private":true}')
subprocess.run(['npm', 'install', '--ignore-scripts', '--no-audit', '--no-fund', 'tsx@4.20.5'],
    cwd=tools, env=env, check=True, timeout=180)
service = app / 'modules/legacy-api'
(service / 'node_modules').symlink_to(tools / 'node_modules', target_is_directory=True)
(service / 'server.ts').write_text('''import {writeFileSync} from 'node:fs';
if(process.env.FORKLAUNCH_MODE!=='openapi') throw new Error('Only export is expected');
writeFileSync(process.env.FORKLAUNCH_OPENAPI_OUTPUT!,JSON.stringify({openapi:'3.1.0',info:{title:'Legacy API',version:'1'},
 paths:{'/legacy':{get:{operationId:'getLegacy',responses:{'200':{description:'OK',content:{'application/json':{schema:{type:'object',properties:{origin:{type:'string'}},required:['origin']}}}}}}}}}));
''')
run('sync', 'all', '--confirm')
manifest = tomllib.loads(manifest_path.read_text())
projects = {project['name']: project for project in manifest['projects']}
assert {'legacy-api', 'native-api', 'native-models', 'discovered-models'} <= projects.keys()
assert projects['native-models']['metadata']['cratePath'] == 'shared/native-models'
assert source.read_bytes() == original
assert 'shared/native-models' in tomllib.loads(cargo.read_text())['workspace']['members']
assert not (app / 'modules/native-api').exists()
assert not (app / 'modules/native-models').exists()
assert (app / '.forklaunch/contracts/native-api/contract.json').is_file()
assert (app / '.forklaunch/openapi/legacy-api/openapi.json').is_file()
sdk = (app / 'sdks/typescript/src/index.ts').read_text()
assert 'getLegacy:' in sdk and 'nativeApi:' in sdk
assert (app / 'sdks/rust/legacy-api-sdk/Cargo.toml').is_file()
assert (app / 'sdks/rust/native-api-sdk/Cargo.toml').is_file()
run('sdk', 'generate', '--from', 'contract', '--target', 'all', '--check')
subprocess.run(['cargo', 'check', '--workspace'], cwd=app, env=env, check=True, timeout=1800)
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=app, env=env))
subprocess.run(['node', str(repo / 'cli/tests/rust_mcp_service_startup.mjs'),
    str(Path(metadata['target_directory']) / 'debug/native-api')], cwd=app, env=env, check=True, timeout=120)
unknown = app / 'modules/unknown-project'
unknown.mkdir()
failed = subprocess.run([str(binary), 'sync', 'all', '--confirm', '--path', str(app)],
    cwd=root, env=env, capture_output=True, text=True, timeout=180)
assert failed.returncode == 0 and "Skipping 'unknown-project'" in failed.stderr + failed.stdout
assert 'panicked' not in failed.stderr
unknown.rmdir()
# Removal on one side must not classify the other language as orphaned.
import shutil
shutil.rmtree(app / 'shared/native-models')
run('sync', 'all', '--confirm')
projects = {project['name']: project for project in tomllib.loads(manifest_path.read_text())['projects']}
assert 'native-models' not in projects
assert {'legacy-api', 'native-api', 'discovered-models'} <= projects.keys()
assert 'native-models' not in (app / 'crates/native-api/Cargo.toml').read_text()
run('sdk', 'generate', '--from', 'contract', '--target', 'all', '--check')
print('PASS mixed app-wide sync: TypeScript discovery, custom native paths, independent orphan cleanup and both SDK targets', flush=True)
app = root / 'rust-only'
run('init', 'application', 'rust-only', '--language', 'rust', '--sdk-targets', 'rust',
    '-D', 'Rust SDK sync', '-A', 'ForkLaunch', '-L', 'MIT')
run('init', 'service', 'native-api', '--language', 'rust', '-D', 'Native API')
run('sync', 'all', '--confirm')
assert (app / 'sdks/rust/native-api-sdk/Cargo.toml').is_file()
assert not (app / 'sdks/typescript').exists()
run('sdk', 'generate', '--from', 'contract', '--target', 'rust', '--check')
print('PASS sync all honors Rust-only SDK targets without adding a TypeScript workspace', flush=True)
print(f'Mixed sync fixture evidence: {root}', flush=True)
