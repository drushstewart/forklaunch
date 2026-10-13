#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export FORKLAUNCH_TEST_COMPONENT_NAMES=1
exec bash "$SCRIPT_DIR/rust_recursive_sdk_runtime.sh"
