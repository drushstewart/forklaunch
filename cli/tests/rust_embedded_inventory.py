#!/usr/bin/env python3
"""Prove new module HTTP layer files cannot silently disappear from CLI embedding."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MODULES = ("ecommerce",)
with tempfile.TemporaryDirectory(prefix="forklaunch-inventory-") as temporary:
    root = Path(temporary)
    binary = root / "inventory-check"
    subprocess.run(["rustc", "+stable", str(ROOT / "cli/build.rs"), "-o", str(binary)], check=True)
    cli = root / "cli"
    cli.mkdir()

    # Published crates have no adjacent source checkout and need no symlinks.
    subprocess.run([str(binary)], cwd=cli, capture_output=True, check=True)
    implementations = root / "blueprint-rs/crates/implementations"
    for module in MODULES:
        source = implementations / module / f"{module}-axum"
        (source / "src").mkdir(parents=True)
        (source / "Cargo.toml").write_text("[package]\n")
        (source / "src/lib.rs").write_text("")
    missing = subprocess.run([str(binary)], cwd=cli, capture_output=True, text=True)
    assert missing.returncode and "inventory is stale" in missing.stderr
    for module in MODULES:
        source = implementations / module / f"{module}-axum"
        target = cli / "src/templates/rust/module-http" / module
        (target / "src").mkdir(parents=True)
        (target / "Cargo.toml.template").symlink_to(source / "Cargo.toml")
        (target / "src/lib.rs").symlink_to(source / "src/lib.rs")
    subprocess.run([str(binary)], cwd=cli, capture_output=True, check=True)
    ecommerce = implementations / "ecommerce/ecommerce-axum/src"
    (ecommerce / ".env").write_text("private fixture")
    ignored = subprocess.run([str(binary)], cwd=cli, capture_output=True, text=True, check=True)
    assert ".env" not in ignored.stdout
    (ecommerce / "pricing.rs").write_text("")
    added = subprocess.run([str(binary)], cwd=cli, capture_output=True, text=True)
    assert added.returncode and "inventory is stale" in added.stderr
print("PASS packaged source, missing module HTTP file rejection, synchronized inventory and private environment exclusion")
