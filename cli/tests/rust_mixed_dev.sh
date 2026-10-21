#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
export CARGO_INCREMENTAL=0
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-stable}"
export FORKLAUNCH_SKIP_FORMAT=1
export FORKLAUNCH_FRAMEWORK_PATH="${FORKLAUNCH_FRAMEWORK_PATH:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/framework-rs}"
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-0}" != 1 ]; then
  # Each runtime owns and cleans its complete fixture before releasing the
  # shared Cargo cache; let other queued checks run between the two fixtures.
  for runtime in ${FORKLAUNCH_TEST_DEV_RUNTIME:-node bun}; do
    FORKLAUNCH_TEST_DEV_RUNTIME="$runtime" python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "$SCRIPT_DIR/rust_mixed_dev.sh"
  done
  exit 0
fi
for runtime in ${FORKLAUNCH_TEST_DEV_RUNTIME:-node bun}; do
  FORKLAUNCH_TEST_DEV_RUNTIME="$runtime" python3 "$SCRIPT_DIR/rust_mixed_dev.py"
done
