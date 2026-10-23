"""Point a generated app's published ForkLaunch crates at this checkout.

Generated apps without `--framework-path` depend on the published crates, so a
runtime test would otherwise exercise crates.io, not the framework and
blueprint changes under test. This writes `[patch.crates-io]` entries for every
publishable ForkLaunch crate into the app's `.cargo/config.toml` and leaves the
generated manifests untouched. Set FORKLAUNCH_TEST_PUBLISHED_CRATES=1 to skip.

Usage: local_crates_patch.py APP_DIR REPO_ROOT
"""
import os
import sys
import tomllib
from pathlib import Path


def crates(repo: Path):
    for workspace in ('framework-rs', 'blueprint-rs'):
        for manifest in sorted((repo / workspace / 'crates').rglob('Cargo.toml')):
            if 'target' in manifest.parts:
                continue
            package = tomllib.loads(manifest.read_text()).get('package', {})
            name = package.get('name', '')
            if name.startswith('forklaunch-') and package.get('publish', True) is not False:
                yield name, manifest.parent


def main() -> None:
    if os.environ.get('FORKLAUNCH_TEST_PUBLISHED_CRATES') == '1':
        return
    app, repo = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    config = app / '.cargo/config.toml'
    existing = config.read_text() if config.is_file() else ''
    if '[patch.crates-io]' in existing:
        return
    lines = ['', '# Test-only: build against this checkout, not crates.io.', '[patch.crates-io]']
    lines += [f'{name} = {{ path = "{path.as_posix()}" }}' for name, path in crates(repo)]
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(existing + '\n'.join(lines) + '\n')


if __name__ == '__main__':
    main()
