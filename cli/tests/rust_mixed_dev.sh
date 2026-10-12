#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
for runtime in ${FORKLAUNCH_TEST_DEV_RUNTIME:-node bun}; do
  FORKLAUNCH_TEST_DEV_RUNTIME="$runtime" python3 "$SCRIPT_DIR/rust_mixed_dev.py"
done
