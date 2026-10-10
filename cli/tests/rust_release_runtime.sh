#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
if [ -z "$FORKLAUNCH_CLI" ]; then
    cargo build --manifest-path "$REPO_ROOT/cli/Cargo.toml"
    FORKLAUNCH_CLI="$REPO_ROOT/cli/target/debug/forklaunch"
fi
python3 "$SCRIPT_DIR/rust_release_runtime.py" "$FORKLAUNCH_CLI"
