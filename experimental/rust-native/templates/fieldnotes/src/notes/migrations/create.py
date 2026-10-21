#!/usr/bin/env python3
"""Create an unapplied, forward-only service migration without external CLI tools."""
import datetime
import re
import sys
from pathlib import Path


def create(directory: Path, name: str) -> Path:
    if not re.fullmatch(r"[a-z][a-z0-9_]*", name):
        raise ValueError("NAME must be lowercase snake_case, starting with a letter")
    versions = [int(path.name.split("_", 1)[0]) for path in directory.glob("[0-9]*_*.sql")]
    version = max(int(datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%d%H%M%S")), max(versions, default=0) + 1)
    target = directory / f"{version}_{name}.sql"
    with target.open("x") as output:
        output.write("-- Forward migration. Run as the Notes migration owner.\n")
    return target


if __name__ == "__main__":
    try:
        print(create(Path(__file__).resolve().parent, sys.argv[1] if len(sys.argv) == 2 else ""))
    except (ValueError, OSError) as error:
        sys.exit(str(error))
