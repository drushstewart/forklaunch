#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Each unchanged dialect case acquires its own build lock, allowing the other
# runtime gates to progress between cases. The aggregate fails if any case fails.
exec python3 "$SCRIPT_DIR/rust_schema_resources_runtime.py"
