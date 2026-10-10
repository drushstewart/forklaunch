#!/usr/bin/env python3
"""Real Cargo/PNPM/Node release builds with an owned platform fixture; no publication."""
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

binary = Path(sys.argv[1]).resolve()
app_id = '00000000-0000-4000-8000-000000000003'
requests = []
class Platform(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_GET(self):
        requests.append(self.path)
        payload = json.dumps({'id':app_id} if self.path.startswith('/applications/') else {'isActive':True}).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
    def do_POST(self):
        requests.append('UNEXPECTED POST')
        self.send_error(500)

server = ThreadingHTTPServer(('127.0.0.1',0),Platform)
thread = threading.Thread(target=server.serve_forever,daemon=True)
thread.start()
try:
    with tempfile.TemporaryDirectory(prefix='forklaunch-generated-release-') as temporary:
        root = Path(temporary).resolve()
        app = root/'app'
        home = root/'private-home'
        (home/'.forklaunch').mkdir(parents=True)
        (home/'.forklaunch/token').write_text(f'access_token = "synthetic-generated-release-token"\nrefresh_token = ""\nexpires_at = {int(time.time())+3600}\n')
        endpoint = f'http://127.0.0.1:{server.server_port}'
        original_home = Path.home()
        env = {**os.environ, 'HOME':str(home),'CI':'true',
               'CARGO_HOME':os.environ.get('CARGO_HOME',str(original_home/'.cargo')),
               'RUSTUP_HOME':os.environ.get('RUSTUP_HOME',str(original_home/'.rustup')),
               'FORKLAUNCH_PLATFORM_MANAGEMENT_API_URL':endpoint,'FORKLAUNCH_BILLING_API_URL':endpoint}
        def cli(*args):
            subprocess.run([str(binary),*args],cwd=root,env=env,check=True,timeout=1800)
        cli('init','application','release-app','--path',str(app),'--language','rust','--sdk-targets','rust','-D','Real release','-A','ForkLaunch','-L','MIT')
        cli('init','service','rust-api','--path',str(app),'--language','rust','-d','postgresql','-D','Rust API')
        manifest = app/'.forklaunch/manifest.toml'
        manifest.write_text(f'platform_application_id = "{app_id}"\n'+manifest.read_text())
        def release(prebuilt=False):
            args = [str(binary),'release','create','--path',str(app),'--local','--dry-run','--skip-sync','--yes','--version','1.0.0']
            if prebuilt:
                args += ['--prebuilt-openapi','.forklaunch/prebuilt','--application-id',app_id]
            subprocess.run(args,cwd=app,env=env,check=True,timeout=1800)
            return json.loads((app/'.forklaunch/release-manifest.json').read_text())
        native = release()
        assert [service['name'] for service in native['services']] == ['rust-api']
        assert '/health' in native['services'][0]['config']['openApiSpec']['paths']
        contract = json.loads((app/'.forklaunch/contracts/rust-api/contract.json').read_text())
        assert contract['generator']=='framework-rs'
        print('PASS real optimized Rust release build and framework contract/OpenAPI export',flush=True)

        cli('sdk','generate','--path',str(app),'--target','all','--from','contract')
        ts = app/'services/ts-api'
        ts.mkdir(parents=True)
        (ts/'package.json').write_text(json.dumps({'name':'ts-api','version':'0.0.0','private':True,'type':'module',
            'scripts':{'build':'tsc --noEmit --target ES2022 --module NodeNext --moduleResolution NodeNext server.ts'},
            'devDependencies':{'typescript':'5.9.3','tsx':'4.20.5','@types/node':'^22'}}))
        (ts/'tsconfig.json').write_text('{"compilerOptions":{"target":"ES2022","module":"NodeNext","moduleResolution":"NodeNext","strict":true}}')
        # A minimal TS export hook exercises the real tsx runtime alongside the
        # generated Rust framework service and generated TypeScript SDK.
        (ts/'server.ts').write_text('''import {writeFileSync} from 'node:fs';
const mode: string | undefined = process.env.FORKLAUNCH_MODE;
if (mode !== 'openapi') throw new Error('Fixture must only export');
writeFileSync(process.env.FORKLAUNCH_OPENAPI_OUTPUT!, JSON.stringify({
  openapi:'3.1.0',info:{title:'TypeScript fixture',version:'1'},
  paths:{'/fixture':{get:{responses:{'200':{description:'OK'}}}}}
}));
''')
        (app/'pnpm-workspace.yaml').write_text('packages:\n  - "sdks/typescript"\n  - "services/*"\n')
        text = manifest.read_text().replace('modules_path = "crates"','modules_path = "services"')
        text = re.sub(r'^language = "rust"$', 'language = "mixed"', text, count=1, flags=re.MULTILINE)
        text = re.sub(r'^applicationLanguage = "rust"$', 'applicationLanguage = "mixed"', text, count=1, flags=re.MULTILINE)
        text += '''\n[[projects]]
name = "ts-api"
type = "Service"
description = "TypeScript fixture"
[projects.metadata]
language = "typescript"
'''
        manifest.write_text(text)
        (app/'.forklaunch/release-manifest.json').unlink()
        mixed = release()
        specs = {service['name']:service['config']['openApiSpec'] for service in mixed['services']}
        assert set(specs)=={'rust-api','ts-api'}
        export_controls = {'FORKLAUNCH_MODE','FORKLAUNCH_OPENAPI_OUTPUT','FORKLAUNCH_CONTRACT_OUTPUT','FORKLAUNCH_ASYNCAPI_OUTPUT'}
        required = {item['name'] for item in mixed.get('requiredEnvironmentVariables',[])}
        assert not export_controls & required, f'Export controls leaked into runtime requirements: {export_controls & required}'
        assert '/health' in specs['rust-api']['paths'] and '/fixture' in specs['ts-api']['paths']
        print('PASS real mixed Cargo/PNPM builds, generated TypeScript SDK, and Rust/tsx shared exports',flush=True)

        for name,spec in specs.items():
            output = app/'.forklaunch/prebuilt'/name/'openapi.json'
            output.parent.mkdir(parents=True)
            output.write_text(json.dumps(spec))
        # Prebuilt preparation must succeed even when these source files cannot
        # compile/export, and honor Rust cratePath outside the TS modules root.
        (app/'crates/rust-api/src/main.rs').write_text('compile_error!("Prebuilt release must not compile application code");\n')
        (ts/'server.ts').write_text('throw new Error("Prebuilt release must not execute application code");\n')
        prebuilt = release(True)
        assert {service['name']:service['config']['openApiSpec'] for service in prebuilt['services']}==specs
        assert not any('synthetic-generated-release-token' in json.dumps(result) for result in [native,mixed,prebuilt])
        assert 'UNEXPECTED POST' not in requests
        print('PASS real prebuilt artifact reuse, mixed crate locations, and no application build/export or publication',flush=True)
finally:
    server.shutdown()
    server.server_close()
    thread.join()
