#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
if [ "${FORKLAUNCH_WORKER_RUNTIME_LOCKED:-}" != 1 ]; then exec python3 "$SCRIPT_DIR/fixtures/worker_runtime_lock.py" "${BASH_SOURCE[0]}" "$@"; fi
CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
RUN_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/rust-sdk-consumer.XXXXXX")"
echo "Rust SDK consumer workspace: $RUN_ROOT"
"$CLI" init application sdk-test --path "$RUN_ROOT/app" --language rust -D SDK -A ForkLaunch -L MIT
"$CLI" init service api --path "$RUN_ROOT/app" --language rust -D API
"$CLI" init library sdk-client --path "$RUN_ROOT/app" --language rust --sdk-services api
cd "$RUN_ROOT/app"
# The consumer must compile before the service has ever run its own build script.
cargo check -p sdk-client
cargo test -p sdk-client
cargo clippy -p sdk-client --all-targets --no-deps -- -D warnings
cargo check --workspace
if "$CLI" init library other-client --path . --language rust --sdk-services api; then echo 'Expected existing SDK consumer preservation';exit 1;fi
"$CLI" change service --path src/api -N renamed-api --confirm
cargo check -p sdk-client
if "$CLI" delete service renamed-api --path . --continue; then echo 'Expected referenced consumer contract preservation';exit 1;fi
test -f src/renamed-api/domain/Cargo.toml
printf '%s\n' 'PASS ordinary Cargo SDK consumer compiles before services and follows service rename'
rm -rf "$RUN_ROOT"
