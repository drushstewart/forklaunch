#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FORKLAUNCH_CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-}}"
run_cli() {
    if [ -n "$FORKLAUNCH_CLI" ]; then "$FORKLAUNCH_CLI" "$@";
    else cargo run --manifest-path "$REPO_ROOT/cli/Cargo.toml" --release -- "$@"; fi
}
mkdir -p "$SCRIPT_DIR/output/rust-generated-otlp-runtime"
RUN_ROOT="$(mktemp -d "$SCRIPT_DIR/output/rust-generated-otlp-runtime/run.XXXXXX")"
run_cli init application telemetry-app --path "$RUN_ROOT/app" --language rust -D 'Generated telemetry' -A ForkLaunch -L MIT
run_cli init service accounts --path "$RUN_ROOT/app" --language rust -d postgresql -D Accounts
run_cli init worker starter --path "$RUN_ROOT/app" --language rust -D Starter
cp -R "$RUN_ROOT/app" "$RUN_ROOT/relocated-app"
cd "$RUN_ROOT/relocated-app"
cargo build --workspace
# Invalid live telemetry settings must not break offline artifact generation.
OTEL_EXPORTER_OTLP_PROTOCOL=invalid OTEL_EXPORTER_OTLP_ENDPOINT=invalid run_cli openapi export --path .
BINARY="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"]+"/debug/accounts")')"
node "$SCRIPT_DIR/rust_generated_otlp_runtime.mjs" "$BINARY"
