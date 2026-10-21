#!/usr/bin/env python3
"""Run a project command with optional dotenv values, without executing shell code.

Accepts KEY=value, optional export, comments, and single/double quoted values.
Variable interpolation and multiline values are intentionally unsupported.
Existing process variables take precedence over file values.
"""
import argparse
import os
from pathlib import Path
import re
import shlex


def read_environment(path: Path) -> dict[str, str]:
    values = {}
    for number, line in enumerate(path.read_text().splitlines(), 1):
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if line.startswith('export '):
            line = line[7:].lstrip()
        key, separator, value = line.partition('=')
        key = key.strip()
        if not separator or not re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*', key):
            raise ValueError(f'{path}:{number}: expected KEY=value')
        try:
            tokens = shlex.split(value, comments=True, posix=True)
        except ValueError:
            raise ValueError(f'{path}:{number}: invalid quoted value') from None
        if len(tokens) > 1:
            raise ValueError(f'{path}:{number}: quote values containing spaces')
        values[key] = tokens[0] if tokens else ''
    return values


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--env-file', default='.env.local')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('a command is required after --')
    path = Path(args.env_file)
    environment = dict(os.environ)
    if path.is_file():
        try:
            environment = {**read_environment(path), **environment}
        except ValueError as error:
            parser.exit(2, f'{error}\n')
    elif args.env_file != '.env.local':
        parser.error(f'environment file does not exist: {path}')
    try:
        os.execvpe(command[0], command, environment)
    except OSError as error:
        parser.exit(1, f'Cannot start {command[0]}: {error.strerror}\n')


if __name__ == '__main__':
    main()
