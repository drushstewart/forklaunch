#!/usr/bin/env python3
"""Prove new preset files cannot silently disappear from CLI embedding."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="forklaunch-inventory-") as temporary:
    root = Path(temporary)
    binary = root / "inventory-check"
    subprocess.run(["rustc", "+stable", str(ROOT / "cli/build.rs"), "-o", str(binary)], check=True)
    cli = root / "cli"
    cli.mkdir()

    # Published crates have no adjacent source checkout and need no symlinks.
    subprocess.run([str(binary)], cwd=cli, capture_output=True, check=True)
    source = root / "experimental/rust-native/templates/fieldnotes"
    source.mkdir(parents=True)
    (source / "006.sql").write_text("select 1;")
    missing = subprocess.run([str(binary)], cwd=cli, capture_output=True, text=True)
    assert missing.returncode and "inventory is stale" in missing.stderr
    target = cli / "src/templates/rust/presets/fieldnotes"
    target.mkdir(parents=True)
    (target / "006.sql").symlink_to(source / "006.sql")
    subprocess.run([str(binary)], cwd=cli, capture_output=True, check=True)
    (source / ".env").write_text("private fixture")
    ignored = subprocess.run([str(binary)], cwd=cli, capture_output=True, text=True, check=True)
    assert ".env" not in ignored.stdout
print("PASS packaged source, missing preset rejection, synchronized inventory and private environment exclusion")
