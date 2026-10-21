"""Retain real raw-init application metadata for explicit legacy compiler fixtures."""
import re
import tomllib
from pathlib import Path


def write_legacy_sdk_manifest(application: Path, destination: Path, project: str):
    source = (application / '.forklaunch/manifest.toml').read_text()
    header = source.split('[[projects]]', 1)[0]
    header = re.sub(r'(?m)^modules_path\s*=.*$', 'modules_path = "crates"', header)
    header = re.sub(r'(?m)^language\s*=.*$', 'language = "rust"', header)
    header = re.sub(r'(?m)^applicationLanguage\s*=.*$', 'applicationLanguage = "rust"', header)
    destination.write_text(header.rstrip() + '\n\n' + project)
    # This explicit historical compiler input keeps the application's
    # `[framework]` selection, so its generated clients depend on the same
    # framework release. It does not require the current CLI to scaffold
    # retired vendored paths.
    framework = tomllib.loads(destination.read_text()).get('framework', {})
    assert framework.get('version') or framework.get('path'), framework
