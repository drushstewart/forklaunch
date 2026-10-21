#!/usr/bin/env python3
"""Compile real built-in module artifacts without running services or databases."""
import argparse
import json
import re
from pathlib import Path
import subprocess
import tempfile

parser=argparse.ArgumentParser()
parser.add_argument('--cli',type=Path,required=True)
args=parser.parse_args()
cli=str(args.cli.resolve())
repo=Path(__file__).resolve().parents[2]
def run(*command,cwd=None):
    result=subprocess.run(command,cwd=cwd,text=True,capture_output=True)
    assert result.returncode==0,(command,result.stdout[-3000:],result.stderr[-8000:])
    return result.stdout
with tempfile.TemporaryDirectory(prefix='rust-module-compliance-') as temporary:
    app=Path(temporary)/'app'
    run(cli,'init','application','module-policy','--path',str(app),'--language','rust','-D','Module policy','-A','ForkLaunch','-L','MIT')
    modules={'iam':'iam-base','cac':'cac-base','ecommerce':'ecommerce-stripe','billing':'billing-base'}
    for name,variant in modules.items():
        run(cli,'init','module',name,'--module',variant,'--language','rust','--path',str(app))
        assert (app/'src'/name/'persistence/compliance_schema.rs').is_file()
    # Upgrade a pre-exporter generated module without replacing user source.
    cac=app/'src/cac'
    (cac/'build.rs').unlink()
    (cac/'persistence/compliance_schema.rs').unlink()
    cargo=(cac/'Cargo.toml').read_text()
    cargo=re.sub(r'\[build-dependencies\][\s\S]*?(?=\n\[|\Z)','',cargo)
    (cac/'Cargo.toml').write_text(cargo)
    with (cac/'lib.rs').open('a') as target:
        target.write('\npub const USER_MARKER: &str = "preserved";\n')
    run(cli,'init','module','cac','--module','cac-base','--language','rust','--path',str(app))
    assert 'USER_MARKER' in (cac/'lib.rs').read_text()
    run('python3',str(repo/'cli/tests/fixtures/worker_runtime_lock.py'),'-c','cargo +stable check --offline --workspace',cwd=app)
    report=json.loads(run(cli,'compliance','validate-artifacts','--path',str(app),'--json'))
    assert len(report['projects'])==6
    declarations={p['project']:p['declarations']['entities'] for p in report['projects']}
    assert declarations['cac']==[] # Actual current mock provider owns no durable store.
    assert all(declarations[name] for name in modules if name!='cac')
    sms=declarations['messaging'][0]
    fields={f['member']:f['classification'] for f in sms['fields']}
    assert fields['to']=='pii' and fields['body']=='pii'
    order=declarations['ecommerce'][0]
    assert next(f for f in order['fields'] if f['member']=='shipping_address')['classification']=='pii'
    assert all(p['encryptionVerification']=='unknown' for p in report['projects'])
print('PASS: six real built-in modules compile, emit owned declarations, preserve TS policy and report runtime unknown')
