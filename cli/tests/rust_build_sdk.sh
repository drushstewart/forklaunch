#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CLI="${FORKLAUNCH_TEST_CLI:-$ROOT/cli/target/debug/forklaunch}"
RUN="$(mktemp -d "${TMPDIR:-/tmp}/forklaunch-build-sdk.XXXXXX")"
trap 'rm -rf "$RUN"' EXIT
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/cli/target}"
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
"$CLI" init application build-sdk --path "$RUN/app" --language rust -D 'Build SDK proof' -A ForkLaunch -L MIT
"$CLI" init service accounts --path "$RUN/app" --language rust -d postgresql -D Accounts
"$CLI" init router records --path "$RUN/app/src/accounts" --language rust
cargo check --manifest-path "$RUN/app/Cargo.toml" -p accounts
python3 - "$RUN/app" <<'PY'
from pathlib import Path
import json,sys
root=Path(sys.argv[1]); sdk=root/'src/client-sdk/typescript/accounts/index.ts'
assert sdk.is_file(), 'cargo check must generate SDK without service startup'
text=sdk.read_text(); assert 'recordCount' in text and 'RecordsResponse' in text
contract=json.loads((sdk.parent/'contract.json').read_text())
assert '/health' in str(contract) and '/records' in str(contract)
p=root/'src/accounts/domain/schemas/mod.rs'
p.write_text(p.read_text().replace('pub struct HealthResponse {\n    pub service: String,','pub struct HealthResponse {\n    pub service: String,\n    pub build_marker: String,',1))
p=root/'src/accounts/domain/services/health.rs'
p.write_text(p.read_text().replace('HealthResponse { service:', 'HealthResponse { build_marker: "generated".to_owned(), service:'))
PY
cargo check --manifest-path "$RUN/app/Cargo.toml" -p accounts
python3 - "$RUN/app" <<'PY'
from pathlib import Path
import sys
sdk=Path(sys.argv[1])/'src/client-sdk/typescript/accounts/index.ts'
assert 'buildMarker' in sdk.read_text(), 'editing shared Rust DTO must regenerate TypeScript during compile'
print('PASS automatic build SDK: initial health/router generation and DTO edit regeneration')
PY
