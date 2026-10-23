#!/usr/bin/env python3
"""TypeScript code-editing commands undo themselves byte for byte.

Scaffolds a Node application with one PostgreSQL service, commits it, and
checks that each change and its inverse (and each add and its delete) returns
the working tree to that commit: no leftover files, keys, dependencies or
scripts, and no reordered ones.

  - change application -r bun / -r node, and -N (which also renames the
    LICENSE copyright holder);
  - infra add / remove of the cache and the object store;
  - init service / worker on another database, then delete;
  - the manifest never gains a top-level snake_case_name.

No package installs; only the CLI runs. Set FORKLAUNCH_CLI to a built binary.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--keep', action='store_true')
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
cli = Path(os.environ.get('FORKLAUNCH_CLI', repo / 'cli/target/release/forklaunch')).resolve()
env = {**os.environ, 'FORKLAUNCH_SKIP_PRECHECK': '1'}
root = Path(tempfile.mkdtemp(prefix='forklaunch-ts-round-trips-'))
base = root / 'base'


def run(arguments, cwd, check=True):
    result = subprocess.run([str(a) for a in arguments], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                            text=True, capture_output=True)
    if check:
        assert result.returncode == 0, ' '.join(map(str, arguments)) + '\n' + result.stdout[-3000:] + result.stderr[-3000:]
    return result


def git(*arguments, cwd):
    return run(['git', *arguments], cwd).stdout


def round_trip(label, *commands, check=None):
    """Run `commands` (each a list of CLI arguments) on a copy of the base and
    require the tree to equal the base commit afterwards."""
    app = root / label
    shutil.copytree(base, app, symlinks=True)
    for command in commands:
        run([cli, *command], app)
        if check:
            check(app, command)
    status = git('status', '--porcelain', '--untracked-files=all', cwd=app)
    assert not status, f'{label} did not round-trip:\n{status}\n' + git('diff', cwd=app)[:6000]
    print(f'PASS {label}', flush=True)


def no_stray_manifest_keys(app, _command):
    manifest = (app / '.forklaunch/manifest.toml').read_text()
    assert not any(line.startswith('snake_case_name') for line in manifest.splitlines()), manifest[:400]


try:
    run([cli, 'init', 'application', 'zebra', '--path', base, '-d', 'postgresql', '-v', 'zod', '-f', 'prettier',
         '-l', 'oxlint', '-F', 'express', '-r', 'node', '-t', 'vitest', '-D', 'Round trips', '-A', 'ForkLaunch',
         '-L', 'MIT', '-o', 'src'], root)
    run([cli, 'init', 'service', 'accounts', '-d', 'postgresql', '-D', 'Accounts', '--path', '.'], base)
    no_stray_manifest_keys(base, None)
    git('init', '-q', cwd=base)
    git('add', '-A', cwd=base)
    git('-c', 'user.email=ci@forklaunch.com', '-c', 'user.name=ci', 'commit', '-qm', 'base', cwd=base)

    def bun_leftovers(app, command):
        if 'bun' in command:
            assert (app / 'src/bunfig.toml').exists() and '"overrides"' in (app / 'src/package.json').read_text()

    round_trip('runtime', ['change', 'application', '-r', 'bun', '-c', '--path', '.'],
               ['change', 'application', '-r', 'node', '-c', '--path', '.'], check=bun_leftovers)

    def license_holder(app, command):
        name = command[command.index('-N') + 1]
        holder = next(line for line in (app / 'src/LICENSE').read_text().splitlines() if line.startswith('Copyright'))
        assert holder.split()[-1] == name, holder

    round_trip('rename', ['change', 'application', '-N', 'kestrel', '-c', '--path', '.'],
               ['change', 'application', '-N', 'zebra', '-c', '--path', '.'], check=license_holder)

    for resource in ['cache', 'object_store']:
        round_trip(f'infra-{resource}', ['infra', 'add', 'accounts', resource, '--path', '.'],
                   ['infra', 'remove', 'accounts', resource, '--path', '.'])

    def core_dependencies(app, command):
        core = (app / 'src/core/package.json').read_text()
        assert core.count('"@mikro-orm/migrations"') <= 1, core
        no_stray_manifest_keys(app, command)

    for database in ['mysql', 'mongodb', 'postgresql']:
        round_trip(f'service-{database}', ['init', 'service', 'ledger', '-d', database, '-D', 'Ledger', '--path', '.'],
                   ['delete', 'service', 'ledger', '-c', '--path', '.'], check=core_dependencies)
    round_trip('worker-mysql', ['init', 'worker', 'jobs', '-t', 'database', '-d', 'mysql', '-D', 'Jobs', '--path', '.'],
               ['delete', 'worker', 'jobs', '-c', '--path', '.'], check=core_dependencies)
    print('PASS TypeScript change round trips', flush=True)
finally:
    if args.keep:
        print('Fixture retained:', root)
    else:
        shutil.rmtree(root, ignore_errors=True)
