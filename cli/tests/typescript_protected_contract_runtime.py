import pathlib,subprocess,os,json,tempfile,shutil
repo=pathlib.Path(__file__).resolve().parents[2];root=pathlib.Path(tempfile.mkdtemp(prefix='forklaunch-protected-contract-'));app=root/'app';cli=repo/'cli/target/debug/forklaunch'
def run(args,cwd=root):
 p=subprocess.run(args,cwd=cwd,text=True,capture_output=True,env={**os.environ,'FORKLAUNCH_SKIP_FORMAT':'1','CARGO_INCREMENTAL':'0'})
 if p.returncode: raise RuntimeError(str(args[:4])+'\n'+p.stdout[-7000:]+'\n'+p.stderr[-7000:])
 return p.stdout
def cmd(*args):return run([str(cli),*args,'--path',str(app)])
try:
 cmd('init','application','protected-proof','--language','typescript','--modules-path','modules','--database','postgresql','--validator','zod','--formatter','prettier','--linter', 'oxlint','--http-framework','express','--runtime','node','--test-framework','vitest','-m','iam-base','-D','Portable auth proof','-A','ForkLaunch','-L','MIT')
 cmd('init','service','records','--language','typescript','-d','postgresql','-D','Protected records')
 service=app/'modules/records'
 for file in ['bootstrapper.ts','server.ts']:
  with (service/file).open('a') as f:f.write('\nthrow new Error("BOOTSTRAP_MUST_NOT_RUN");\n')
 assert (service/'api/contracts/compliance.contract.ts').is_file()
 run(['pnpm','install','--ignore-scripts'],app/'modules')
 env={**os.environ,'CARGO_INCREMENTAL':'0','CARGO_TARGET_DIR':str(repo/'cli/target'),'RUSTUP_TOOLCHAIN':'stable'}
 import shlex
 result=subprocess.run(['python3',str(repo/'cli/tests/fixtures/worker_runtime_lock.py'),'-c',shlex.join(['pnpm','--dir',str(app/'modules'),'build'])],cwd=repo,env=env,text=True,capture_output=True)
 assert result.returncode==0,result.stdout[-5000:]+result.stderr[-7000:]
 print('PASS protected and compliance shared descriptors compile with existing core binder',flush=True)
 cmd('sdk','build','--service','records')
 contract=json.loads((app/'.forklaunch/contracts/records/contract.json').read_text())
 assert len(contract['routes'])==4,contract
 for route in contract['routes']:
  assert route['auth']['method']=='jwt',route
  assert route['access']=='protected'
  assert route['rbac']['allowedRoles']==(['system'] if route['path'].startswith('/compliance') else ['admin'])
 raw=(app/'.forklaunch/contracts/records/contract.json').read_text()
 assert 'jwksPublicKeyUrl' not in raw and 'signatureKey' not in raw
 docs=json.loads((app/'.forklaunch/openapi/records/openapi.json').read_text())
 for doc in docs.values():
  for item in doc['paths'].values():
   for operation in item.values():
    assert operation['security'] and operation['x-forklaunch-auth']['scheme']=='jwt'
 print('PASS isolated protected/compliance export preserves scheme and role policies without key material',flush=True)
 controller=service/'api/controllers/records.controller.ts'
 original=controller.read_text()
 before=(app/'.forklaunch/contracts/records/contract.json').read_bytes()
 controller.write_text(original.replace('jwksPublicKeyUrl: JWKS_PUBLIC_KEY_URL', "jwksPublicKeyUrl: JWKS_PUBLIC_KEY_URL, allowedRoles: ['guest']"))
 rejected=subprocess.run([str(cli),'sdk','build','--skip-compile','--service','records','--path',str(app)],capture_output=True,text=True)
 assert rejected.returncode!=0 and 'credentials and resource decoding only' in rejected.stderr,rejected.stderr
 assert (app/'.forklaunch/contracts/records/contract.json').read_bytes()==before
 controller.write_text(original)
 print('PASS runtime policy override rejected before evaluation; previous SDK contract retained',flush=True)

finally:
 if os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):print('Fixture retained:',root,flush=True)
 else:shutil.rmtree(root)
