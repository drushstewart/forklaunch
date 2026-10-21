#!/usr/bin/env python3
"""Mirror the blueprint ignore files that CLI project templates embed.

Generated Rust modules bind handlers from published blueprint crates, so the
CLI embeds no blueprint Rust sources. Git refuses symlink ignore files, so the
embedded copies are kept identical to their blueprint owners instead.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


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


def sync():
    # Cargo's Git status walker rejects symlink ignore files even when the
    # template link resolves. Mirror every catalog's owned ignore file.
    for source in (ROOT / 'blueprint').glob('*/.gitignore'):
        target = ROOT / 'cli/src/templates/project' / source.parent.name / '.gitignore'
        if target.exists() or target.is_symlink():
            owner = {'billing-stripe': 'billing-base', 'messaging-twilio': 'messaging-base'}.get(source.parent.name, source.parent.name)
            source = ROOT / 'blueprint' / owner / '.gitignore'
            mirror_git_metadata(source, target)


if __name__ == '__main__':
    sync()
