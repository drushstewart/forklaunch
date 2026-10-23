"""Retain real raw-init application metadata for explicit legacy compiler fixtures."""
import json
import re
import tomllib
from pathlib import Path


def write_legacy_sdk_manifest(application: Path, destination: Path, project: str):
    source = (application / '.forklaunch/manifest.toml').read_text()
    header = source.split('[[projects]]', 1)[0]
    header = re.sub(r'(?m)^modules_path\s*=.*$', 'modules_path = "crates"', header)
    header = re.sub(r'(?m)^language\s*=.*$', 'language = "rust"', header)
    header = re.sub(r'(?m)^applicationLanguage\s*=.*$', 'applicationLanguage = "rust"', header)
    text = header.rstrip() + '\n\n' + project
    # The CLI writes top-level tables ([framework], [project_peer_topology], ...)
    # after [[projects]]; carry every application-level table over verbatim.
    sections = re.split(r'(?m)^(?=\[)', source.split('[[projects]]', 1)[1] if '[[projects]]' in source else '')
    for section in sections:
        header_line = section.split('\n', 1)[0].strip()
        if header_line.startswith('[') and not header_line.startswith(('[[projects', '[projects')):
            text = text.rstrip() + '\n\n' + section.rstrip() + '\n'
    destination.write_text(text)
    # This explicit historical compiler input keeps the application's
    # `[framework]` selection, so its generated clients depend on the same
    # framework release. It does not require the current CLI to scaffold
    # retired vendored paths.
    framework = tomllib.loads(destination.read_text()).get('framework', {})
    assert framework.get('version') or framework.get('path'), framework
