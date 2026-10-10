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
    "src/templates/rust/application/typescript-sdk-tsconfig.json",
    "src/templates/rust/project/Cargo.toml.template",
    "src/templates/rust/iam/Cargo.toml.template",
    "src/templates/rust/leased-worker/lib.rs",
    "src/templates/rust/leased-worker/smoke.rs",
    "src/templates/rust/leased-worker/signals.rs",
    "src/templates/rust/kafka-worker/main.rs",
    "src/templates/rust/kafka-worker/signals.rs",
    "src/templates/rust/kafka-worker/handler.rs",
    "src/templates/rust/kafka-worker/env.example",
    "src/templates/rust/kafka-worker/README.md",
    "src/templates/rust/postgres-worker/main.rs",
    "src/templates/rust/postgres-worker/handler.rs",
    "src/templates/rust/postgres-worker/env.example",
    "src/templates/rust/postgres-worker/README.md",
    "src/templates/rust/redis-worker/main.rs",
    "src/templates/rust/redis-worker/handler.rs",
    "src/templates/rust/redis-worker/env.example",
    "src/templates/rust/redis-worker/README.md",
    "src/templates/rust/blueprint-workspace/Cargo.toml.template",
    "src/templates/rust/framework/Cargo.toml.template",
}
for workspace, namespace in [("framework-rs", "framework"), ("blueprint-rs", "blueprint-workspace")]:
    for source in (root / workspace / "crates").rglob("*"):
        if source.is_file():
            relative = source.relative_to(root / workspace)
            if source.name == "Cargo.toml":
                relative = relative.with_name("Cargo.toml.template")
            expected.add(str(Path("src/templates/rust") / namespace / relative))
missing = expected - packaged
assert not missing, f"Rust sources missing from CLI package: {sorted(missing)}"
print(f"PASS: {len(expected)} embedded Rust source/template files included in Cargo package")
PYTHON
