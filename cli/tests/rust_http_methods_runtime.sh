#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/cli/target}"
export CARGO_INCREMENTAL=0 RUSTUP_TOOLCHAIN=stable
export FORKLAUNCH_FRAMEWORK_PATH="${FORKLAUNCH_FRAMEWORK_PATH:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/framework-rs}" FORKLAUNCH_SKIP_FORMAT=1
export FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$CARGO_TARGET_DIR/debug/forklaunch}}"
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-}" != 1 ]; then
  exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "${BASH_SOURCE[0]}" "$@"
fi
exec python3 "$SCRIPT_DIR/rust_http_methods_runtime.py"
