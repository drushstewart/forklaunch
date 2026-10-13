#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export FORKLAUNCH_TEST_ASYNCAPI_RESOURCES=1
bash "$SCRIPT_DIR/rust_asyncapi_import_runtime.sh"
