#!/usr/bin/env python3
"""Expose the Rust module HTTP layers to include_dir and cargo package.

Links keep the source authoritative in this checkout. Cargo packages dereference
these files; generated applications never depend on links into the CLI checkout.
"""
from pathlib import Path
import os

ROOT = Path(__file__).resolve().parents[1]
EMBEDDED = ROOT / 'cli/src/templates/rust'
SKIP = {'target', 'node_modules', '.git', '.DS_Store', '__pycache__', 'dist', 'artifacts', 'secrets'}
GIT_METADATA = {'.gitignore', '.gitattributes'}
MODULE_HTTP = ('iam', 'billing', 'cac', 'ecommerce')


def mirror_git_metadata(source: Path, target: Path):
    """Git refuses symlink ignore/attribute files; these copies are owned catalogs."""
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.is_symlink():
        if target.resolve() != source.resolve():
            raise ValueError(f'Preserving unexpected Git metadata link: {target}')
        target.unlink()
    content = source.read_bytes()
    if not target.exists() or target.read_bytes() != content:
        target.write_bytes(content)


def link(source: Path, target: Path):
    target.parent.mkdir(parents=True, exist_ok=True)
    if source.name in GIT_METADATA:
        mirror_git_metadata(source, target)
        return
    relative = os.path.relpath(source, target.parent)
    if target.is_symlink() and os.readlink(target) == relative:
        return
    if target.exists() or target.is_symlink():
        raise ValueError(f'Preserving unexpected embedded file: {target}')
    target.symlink_to(relative)


def name(path: Path):
    return path.name + '.template' if path.name in {'Cargo.toml', 'Cargo.lock'} else path.name


def source_name(value: str) -> bool:
    return (value not in SKIP and not value.endswith(('.pyc', '.pyo', '.local'))
            and (not value.startswith('.env') or value.endswith('.example')))


def sync_tree(source: Path, target: Path, *, rename_manifests=True, ancestors=()):
    """Replace only exact owned directory links; never traverse/delete source state."""
    resolved = source.resolve()
    if resolved in ancestors:
        raise ValueError(f'Source directory cycle: {source}')
    if target.is_symlink():
        if target.resolve() != resolved:
            raise ValueError(f'Preserving unexpected catalog directory link: {target}')
        target.unlink()  # Unlink catalog only; the authoritative source remains intact.
    target.mkdir(parents=True, exist_ok=True)
    expected = set()
    for child in sorted(source.iterdir()):
        if not source_name(child.name):
            continue
        filename = name(child) if rename_manifests else child.name
        expected.add(filename)
        destination = target / filename
        if child.is_dir():
            sync_tree(child, destination, rename_manifests=rename_manifests,
                      ancestors=(*ancestors, resolved))
        elif child.is_file():
            link(child, destination)
        else:
            raise ValueError(f'Unsupported source entry: {child}')
    for extra in target.iterdir():
        if extra.name in expected:
            continue
        original_name = extra.name.removesuffix('.template') if rename_manifests else extra.name
        owned_source = source / original_name
        if extra.is_symlink() and extra.resolve() == owned_source.resolve():
            extra.unlink()  # Obsolete or now-excluded generator-owned file/link only.
        elif extra.is_dir() and not extra.is_symlink() and not any(extra.iterdir()):
            # Empty old catalog directories contain no user files; retain harmlessly.
            continue
        else:
            raise ValueError(f'Preserving unexpected embedded entry: {extra}')


def sync():
    # Applications install the published @forklaunch/universal-sdk; the CLI
    # no longer embeds its sources.
    # Cargo's Git status walker rejects symlink ignore files even when the
    # template link resolves. Mirror every catalog's owned ignore file.
    for source in (ROOT / 'blueprint').glob('*/.gitignore'):
        target = ROOT / 'cli/src/templates/project' / source.parent.name / '.gitignore'
        if target.exists() or target.is_symlink():
            owner = {'billing-stripe': 'billing-base', 'messaging-twilio': 'messaging-base'}.get(source.parent.name, source.parent.name)
            source = ROOT / 'blueprint' / owner / '.gitignore'
            mirror_git_metadata(source, target)
    # Module HTTP layers are the only blueprint source generated applications
    # receive; every other framework and blueprint crate comes from crates.io.
    for module in MODULE_HTTP:
        source = ROOT / 'blueprint-rs/crates/implementations' / module / f'{module}-axum'
        target = EMBEDDED / 'module-http' / module
        link(source / 'Cargo.toml', target / 'Cargo.toml.template')
        sync_tree(source / 'src', target / 'src')


if __name__ == '__main__':
    sync()
