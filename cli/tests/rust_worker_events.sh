#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
TSC="${FORKLAUNCH_TEST_TSC:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
test -x "$CLI"
test -x "$TSC"
RUN_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/worker-events.XXXXXX")"
trap 'rm -rf "$RUN_ROOT"' EXIT
"$CLI" init application events-app --path "$RUN_ROOT/app" --language rust -D Events -A ForkLaunch -L MIT
"$CLI" init worker tasks --path "$RUN_ROOT/app" --language rust -t redis -D Tasks
cargo test --manifest-path "$RUN_ROOT/app/Cargo.toml" -p tasks --lib
node "$SCRIPT_DIR/rust_worker_events.mjs" "$RUN_ROOT/app/src/tasks" "$TSC"
# Output watches recover files after a cached build; schemas follow edited Rust fields.
rm "$RUN_ROOT/app/src/tasks/events/event-contract.json"
cargo build --manifest-path "$RUN_ROOT/app/Cargo.toml" -p tasks
[ -s "$RUN_ROOT/app/src/tasks/events/event-contract.json" ]
python3 - "$RUN_ROOT/app/src/tasks/domain/schemas/event_contract.rs" <<'PY'
from pathlib import Path
import sys
p=Path(sys.argv[1]);p.write_text(p.read_text().replace('pub message: String,','pub message: String,\n    pub importance: bool,'))
PY
cargo build --manifest-path "$RUN_ROOT/app/Cargo.toml" -p tasks
node --input-type=module - "$RUN_ROOT/app/src/tasks/events/event-contract.json" <<'JS'
import fs from 'node:fs';
const schema=JSON.parse(fs.readFileSync(process.argv[2], 'utf8')).input;
if(schema.properties.importance.type !== 'boolean' || !schema.required.includes('importance')) throw Error('edited Rust field was not generated');
JS
echo 'PASS Rust schema changes and deleted worker event outputs regenerate on compilation'
