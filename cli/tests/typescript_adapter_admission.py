#!/usr/bin/env python3
"""Adapter admission is read-only and uses the requested runtime/package policy."""
import hashlib, json, os, pathlib, shutil, subprocess, tempfile
repo = pathlib.Path(__file__).resolve().parents[2]
cli = pathlib.Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/debug/forklaunch'))
root = pathlib.Path(tempfile.mkdtemp(prefix='forklaunch-adapter-admission-'))
env = {**os.environ, 'FORKLAUNCH_SKIP_FORMAT': '1'}
def command(app, *args, check=True):
    result = subprocess.run([str(cli), *args, '--path', str(app)], env=env, text=True, capture_output=True)
    if check: assert result.returncode == 0, result.stdout + result.stderr
    return result
try:
    for runtime in ('node', 'bun'):
        app = root / runtime
        command(app, 'init', 'application', 'admission', '--language', 'typescript', '--modules-path', 'modules', '--database', 'postgresql', '--validator', 'zod', '--formatter', 'prettier', '--linter', 'eslint', '--http-framework', 'express', '--runtime', runtime, '--test-framework', 'vitest', '-D', 'Admission', '-A', 'ForkLaunch', '-L', 'MIT')
        command(app, 'init', 'service', 'api', '--language', 'typescript', '-d', 'postgresql', '-D', 'API')
        if runtime == 'node':
            package_path = app / 'modules/package.json'
            package = json.loads(package_path.read_text())
            package['scripts']['clean:purge'] = 'node -e "require(\'node:fs\').writeFileSync(\'purged\', \'unexpected\')"'
            package_path.write_text(json.dumps(package, indent=2) + '\n')
            def tree(): return {str(p.relative_to(app)): hashlib.sha256(p.read_bytes()).hexdigest() for p in app.rglob('*') if p.is_file()}
            original = tree()
            result = command(app, 'change', 'application', '-N', 'renamed', '--http-framework', 'hyper-express', '--confirm', check=False)
            assert result.returncode != 0 and 'pnpm policy blocks' in result.stdout + result.stderr
            assert tree() == original, 'Rejected adapter conversion mutated files or executed cleanup'
        else:
            assert not (app / 'modules/pnpm-workspace.yaml').exists()
            command(app, 'change', 'application', '--runtime', 'node', '--http-framework', 'hyper-express', '--confirm')
            assert json.loads((app / 'modules/package.json').read_text())['packageManager'] == 'pnpm@10.33.2'
    print('PASS rejected HyperExpress+rename preserves full tree and never purges; fresh Bun-to-Node admission selects compatible package manager')
finally:
    shutil.rmtree(root)
