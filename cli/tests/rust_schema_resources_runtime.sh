#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export FORKLAUNCH_TEST_SCHEMA_RESOURCES=1
bash "$SCRIPT_DIR/rust_recursive_sdk_runtime.sh"
