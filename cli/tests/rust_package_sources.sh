#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PACKAGE_LIST="$(mktemp)"
trap 'rm -f "$PACKAGE_LIST"' EXIT
cargo package --manifest-path "$REPO_ROOT/cli/Cargo.toml" --list --allow-dirty > "$PACKAGE_LIST"
python3 - "$REPO_ROOT" "$PACKAGE_LIST" <<'PYTHON'
from pathlib import Path
import sys

root = Path(sys.argv[1])
packaged = set(Path(sys.argv[2]).read_text().splitlines())
expected = {
    "src/templates/rust/application/Cargo.toml.template",
    "src/templates/rust/project/Cargo.toml.template",
    "src/templates/rust/framework/Cargo.toml.template",
}
for source in (root / "framework-rs/crates").rglob("*"):
    if source.is_file():
        relative = source.relative_to(root / "framework-rs")
        if source.name == "Cargo.toml":
            relative = relative.with_name("Cargo.toml.template")
        expected.add(str(Path("src/templates/rust/framework") / relative))
missing = expected - packaged
assert not missing, f"Rust sources missing from CLI package: {sorted(missing)}"
print(f"PASS: {len(expected)} embedded Rust source/template files included in Cargo package")
PYTHON
