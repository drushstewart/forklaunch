#!/usr/bin/env python3
"""Fresh Bun build/watch emits both SDKs without evaluating runtime modules."""
import json, os, pathlib, shlex, shutil, subprocess, sys, tempfile, time
repo = pathlib.Path(__file__).resolve().parents[2]
cli = pathlib.Path(os.environ.get('FORKLAUNCH_CLI', repo/'cli/target/debug/forklaunch'))
root = pathlib.Path(tempfile.mkdtemp(prefix='forklaunch-bun-contract-'))
app = root/'app'
env = {**os.environ, 'CARGO_INCREMENTAL':'0', 'CARGO_TARGET_DIR':str(repo/'cli/target'), 'RUSTUP_TOOLCHAIN':'stable', 'FORKLAUNCH_SKIP_FORMAT':'1'}
def run(args, cwd=app, environment=env):
    result = subprocess.run(args, cwd=cwd, env=environment, text=True, capture_output=True)
    assert result.returncode == 0, result.stdout[-5000:] + result.stderr[-9000:]
    return result.stdout
def command(*args):
    return run([str(cli), *args, '--path', str(app)], root)
def locked(args):
    return run([sys.executable, str(repo/'cli/tests/fixtures/worker_runtime_lock.py'), '-c', shlex.join(args)], repo)
try:
    command('init','application','bun-contract-proof','--language','typescript','--modules-path','modules','--database','postgresql','--validator','zod','--formatter','prettier','--linter','eslint','--http-framework','express','--runtime','bun','--test-framework','vitest','-D','Bun proof','-A','ForkLaunch','-L','MIT')
    # Contract builds exist for Rust consumers; a TypeScript-only app keeps tsc -b.
    command('init', 'service', 'consumer', '--language', 'rust', '-D', 'Rust consumer')
    command('init','service','notes','--language','typescript','-d','postgresql','-D','Notes')
    command('init','worker','jobs','--language','typescript','-t','database','-d','postgresql','-D','Jobs')
    for name in ['notes','jobs']:
        service = app/'modules'/name
        assert json.loads((service/'package.json').read_text())['scripts']['build'].startswith('bun ')
        for filename in ['server.ts','bootstrapper.ts']:
            with (service/filename).open('a') as f: f.write('\nthrow new Error("BUN_RUNTIME_MUST_NOT_BE_IMPORTED");\n')
    run(['bun','install','--ignore-scripts'], app/'modules')
    sample = app/'modules/notes/runtime-selection.test.ts'
    sample.write_text("import { expect, test } from 'vitest';\ntest('selected runtime', () => { expect(typeof Bun).toBe('object'); });\n")
    for directory in [app/'modules', app/'modules/notes', app/'modules/jobs']:
        assert json.loads((directory/'package.json').read_text())['scripts']['test'] == 'bun run --bun vitest --passWithNoTests'
    run(['bun','run','test','--run','notes/runtime-selection.test.ts'], app/'modules')
    run(['bun','run','test','--run','runtime-selection.test.ts'], app/'modules/notes')
    sample.unlink()
    print('PASS Bun root and project test scripts run selected Vitest on Bun', flush=True)
    locked(['bun','run','--cwd',str(app/'modules'),'build'])
    for name in ['notes','jobs']:
        config = json.loads((app / '.forklaunch/build.json').read_text())['contracts'][name]
        assert len(json.loads((app/config['contract']).read_text())['routes']) == 2
        assert all((app/config[key]).is_file() for key in ['typescript','rust','openapi'])
    print('PASS fresh Bun normal service/worker build emits both SDKs without bootstrap evaluation', flush=True)
    # Export must use Bun's own loader, with no Node executable available.
    binaries=root/'bun-only';binaries.mkdir();(binaries/'bun').symlink_to(shutil.which('bun'))
    run([str(cli),'sdk','build','--skip-compile','--service','notes','--path',str(app)], root, {**env,'PATH':str(binaries)})
    print('PASS isolated Bun export with Node absent from PATH', flush=True)
    schema=app/'modules/notes/domain/schemas/notes.schema.ts';original=schema.read_text()
    config=json.loads((app / '.forklaunch/build.json').read_text())['contracts']['notes']
    with (root/'watch.log').open('w') as output:
        watcher=subprocess.Popen(['bun',str(app/'.forklaunch/tools/contract-build.mjs'),'notes','--watch'],cwd=app,env=env,stdout=output,stderr=output)
        try:
            time.sleep(3);schema.write_text(original.replace('message: string','message: string.min(3)'))
            deadline=time.monotonic()+120
            while time.monotonic()<deadline:
                if all('minLength' in (app/config[key]).read_text() for key in ['typescript','rust']):break
                assert watcher.poll() is None,(root/'watch.log').read_text()
                time.sleep(.3)
            else:raise AssertionError((root/'watch.log').read_text())
        finally:
            watcher.terminate();watcher.wait(timeout=15)
    print('PASS Bun normal watch updates TypeScript and Rust SDKs',flush=True)
    docker=(app/'Dockerfile').read_text()
    assert 'FROM oven/bun:1.2.19 AS runtime' in docker and 'contract-compiler' in docker and 'pnpm' not in docker
    assert (app/'modules/bun.lock').is_file()
finally:
    if os.environ.get('FORKLAUNCH_KEEP_FIXTURE'):print('Fixture retained:',root,flush=True)
    else:shutil.rmtree(root)
