#!/usr/bin/env python3
"""Actual release CLI with owned platform/tool fixtures; no publication or live account."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

binary = Path(sys.argv[1]).resolve()
app_id = '00000000-0000-4000-8000-000000000002'
requests = []

class Platform(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_GET(self):
        requests.append(self.path)
        payload = json.dumps({'id': app_id} if self.path.startswith('/applications/') else {'isActive': True}).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
    def do_POST(self):
        requests.append('UNEXPECTED POST')
        self.send_error(500)

server = ThreadingHTTPServer(('127.0.0.1', 0), Platform)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
try:
    with tempfile.TemporaryDirectory(prefix='forklaunch-rust-release-') as temporary:
        root = Path(temporary).resolve()
        home = root/'private-home'
        (home/'.forklaunch').mkdir(parents=True)
        (home/'.forklaunch/token').write_text(f'access_token = "synthetic-release-test-token"\nrefresh_token = ""\nexpires_at = {int(time.time())+600}\n')
        tools = root/'tools'
        tools.mkdir()
        tool_source = '''import json,os,sys
from pathlib import Path
name=Path(sys.argv[0]).name
args=sys.argv[1:]
with open(os.environ['RELEASE_TEST_LOG'],'a') as log:
    log.write(json.dumps({'program':name,'args':args,'cwd':os.getcwd(),'mode':os.getenv('FORKLAUNCH_MODE')})+'\\n')
stage='cargo' if name=='cargo' and args[0]=='build' else 'install' if args[0]=='install' else 'javascript'
if os.getenv('RELEASE_TEST_FAIL')==stage: sys.exit(91)
if os.getenv('FORKLAUNCH_MODE') in ['contract','openapi']:
    output=Path(os.environ['FORKLAUNCH_OPENAPI_OUTPUT'])
    output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps({'openapi':'3.1.0','info':{'title':Path.cwd().name if name!='cargo' else args[2],'version':'1'},'paths':{}}))
    if 'FORKLAUNCH_CONTRACT_OUTPUT' in os.environ:
        contract=Path(os.environ['FORKLAUNCH_CONTRACT_OUTPUT'])
        contract.parent.mkdir(parents=True,exist_ok=True)
        contract.write_text(json.dumps({'generator':'framework-rs','service':args[2],'routes':[]}))
'''
        for name in ['cargo','pnpm','bun','node']:
            path = tools/name
            path.write_text(f'#!{sys.executable}\n'+tool_source)
            path.chmod(0o700)
        endpoint = f'http://127.0.0.1:{server.server_port}'
        for scenario in ['rust','mixed','sdk','fail-cargo','fail-install','fail-javascript','prebuilt','prebuilt-escape']:
            app = root/scenario
            rust = app/'crates/rust-api'
            rust.mkdir(parents=True)
            (app/'.forklaunch').mkdir()
            mixed = scenario not in ['rust','sdk']
            manifest = f'''id = "release-proof"
cli_version = "0.0.0"
app_name = "release-proof"
modules_path = "{'services' if mixed else 'crates'}"
app_description = "Release fixture"
linter = "clippy"
formatter = "rustfmt"
validator = "serde"
http_framework = "axum"
runtime = "tokio"
author = "QA"
license = "MIT"
language = "{'mixed' if mixed else 'rust'}"
applicationLanguage = "{'mixed' if mixed else 'rust'}"
cargoWorkspace = "Cargo.toml"
platform_application_id = "{app_id}"
project_peer_topology = {{}}
[[projects]]
name = "rust-api"
type = "Service"
description = "Rust API"
[projects.metadata]
language = "rust"
cratePath = "crates/rust-api"
contractArtifactPath = ".forklaunch/contracts/rust-api/contract.json"
'''
            (rust/'Cargo.toml').write_text('[package]\nname="rust-api"\nversion="0.1.0"\nedition="2024"\n')
            (app/'Cargo.toml').write_text('[workspace]\nmembers=["crates/rust-api"]\n')
            if mixed:
                ts = app/'services/ts-api'
                (ts/'node_modules/tsx/dist').mkdir(parents=True)
                (ts/'node_modules/tsx/dist/cli.mjs').write_text('// tool fixture\n')
                (ts/'package.json').write_text('{"name":"ts-api","version":"1.0.0"}')
                (ts/'tsconfig.json').write_text('{}')
                (ts/'server.ts').write_text('// Export is simulated by the owned Node fixture.\n')
                manifest += '''[[projects]]
name = "ts-api"
type = "Service"
description = "TypeScript API"
[projects.metadata]
language = "typescript"
'''
            if scenario != 'rust':
                (app/'package.json').write_text(json.dumps({'private':True,'scripts':{} if scenario=='sdk' else {'build':'owned-build-fixture'}}))
                (app/'pnpm-lock.yaml').write_text("lockfileVersion: '9.0'\n")
            if scenario == 'prebuilt-escape':
                (root/'outside').mkdir()
                manifest = manifest.replace('cratePath = "crates/rust-api"','cratePath = "../outside"')
            (app/'.forklaunch/manifest.toml').write_text(manifest)
            log = app/'tool-log.jsonl'
            env = {**os.environ, 'HOME':str(home),'PATH':str(tools)+':/usr/bin:/bin',
                   'CI':'true','RELEASE_TEST_LOG':str(log),
                   'FORKLAUNCH_PLATFORM_MANAGEMENT_API_URL':endpoint,'FORKLAUNCH_BILLING_API_URL':endpoint}
            args = [str(binary),'release','create','--local','--dry-run','--skip-sync','--yes','--version','1.0.0']
            if scenario.startswith('fail-'):
                env['RELEASE_TEST_FAIL'] = scenario.removeprefix('fail-')
            if scenario.startswith('prebuilt'):
                for name in ['rust-api','ts-api']:
                    output = app/'.forklaunch/openapi'/name/'openapi.json'
                    output.parent.mkdir(parents=True)
                    output.write_text('{"openapi":"3.1.0","info":{"title":"Prebuilt","version":"1"},"paths":{}}')
                args += ['--prebuilt-openapi','.forklaunch/openapi','--application-id',app_id]
            result = subprocess.run(args,cwd=app,env=env,capture_output=True,text=True,timeout=30)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            release_path = app/'.forklaunch/release-manifest.json'
            if scenario == 'prebuilt-escape':
                assert result.returncode != 0 and not release_path.exists(), 'Escaping cratePath accepted'
                assert not calls, 'Invalid snapshot executed tools'
                continue
            if scenario.startswith('fail-'):
                assert result.returncode != 0 and not release_path.exists(), result.stdout+result.stderr
                assert len(calls) == {'fail-cargo':1,'fail-install':2,'fail-javascript':3}[scenario], calls
                assert not any(call['mode'] for call in calls), 'Export ran after build failure'
                continue
            assert result.returncode == 0, scenario+'\n'+result.stdout+result.stderr
            release = json.loads(release_path.read_text())
            assert {service['name'] for service in release['services']} == ({'rust-api','ts-api'} if mixed else {'rust-api'})
            assert all(service['config']['openApiSpec'] for service in release['services']), release
            assert 'synthetic-release-test-token' not in release_path.read_text()
            if scenario.startswith('prebuilt'):
                assert not calls, 'Prebuilt release executed a package/build/export tool'
                continue
            assert calls[0]['program']=='cargo' and calls[0]['args']==['build','--workspace','--release']
            assert calls[0]['cwd']==str(app)
            contract = json.loads((app/'.forklaunch/contracts/rust-api/contract.json').read_text())
            assert contract['service']=='rust-api'
            if scenario == 'rust':
                assert [call['program'] for call in calls]==['cargo','cargo'], calls
            else:
                assert calls[1]['program']=='pnpm' and calls[1]['args']==['install','--frozen-lockfile']
                assert calls[2]['program']=='pnpm'
                assert calls[2]['args']==(['--recursive','--if-present','run','build'] if scenario=='sdk' else ['build'])
                assert calls[1]['cwd']==calls[2]['cwd']==str(app)
                if mixed:
                    assert [call['program'] for call in calls]==['cargo','pnpm','pnpm','cargo','node'], calls
        assert 'UNEXPECTED POST' not in requests, 'Dry run published to the platform'
        print('PASS Rust/mixed release build order, SDK package builds, shared exports, failure stops, and prebuilt isolation')
finally:
    server.shutdown()
    server.server_close()
    thread.join()
