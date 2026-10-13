#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for mode in static legacy mixed dynamic recursive fragment4 fragment6 fragment7 siblings composed uri4 uri6 uri7 pointer4 pointer6 pointer7; do
 export FORKLAUNCH_TEST_SCHEMA_RESOURCES="$mode"
 bash "$SCRIPT_DIR/rust_recursive_sdk_runtime.sh"
done
