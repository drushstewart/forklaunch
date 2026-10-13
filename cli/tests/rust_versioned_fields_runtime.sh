#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
mkdir -p "$REPO_ROOT/output/rust-versioned-fields"
RUN_ROOT="$(mktemp -d "$REPO_ROOT/output/rust-versioned-fields/run.XXXXXX")"
"$CLI" init application fields-app --path "$RUN_ROOT/app" --language rust -D Fields -A ForkLaunch -L MIT
"$CLI" init service fields --path "$RUN_ROOT/app" --language rust -d postgresql -D Fields
mv "$RUN_ROOT/app" "$RUN_ROOT/relocated"
cd "$RUN_ROOT/relocated"
cp "$SCRIPT_DIR/fixtures/rust_version_fields.rs" crates/fields/src/fields_fixture.rs
python3 - <<'PY'
from pathlib import Path
p=Path('crates/fields/Cargo.toml');p.write_text(p.read_text().replace('[dependencies]\n','[dependencies]\nserde_json = "1"\n'))
p=Path('crates/fields/src/lib.rs');s='pub mod fields_fixture;\n'+p.read_text();s=s.replace('for contract in routes::contracts() {','for contract in routes::contracts().into_iter().chain(fields_fixture::contracts()) {');p.write_text(s)
p=Path('crates/fields/src/main.rs');p.write_text(p.read_text().replace('router(state).serve_with_default_mcp','fields::fields_fixture::register(router(state)).serve_with_default_mcp'))
PY
cargo build -p fields
"$CLI" openapi export --path .
for artifact in contract openapi; do
    "$CLI" sdk generate --path . --target all --from "$artifact"
    "${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}" --project sdks/typescript/tsconfig.json --noEmit
    cargo check -p fields-sdk
done
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/fields"))')"
node "$SCRIPT_DIR/rust_versioned_fields_runtime.mjs" "$BINARY"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY"
echo 'PASS generated versioned field transforms/defaults, raw wire preservation, both SDK compilers and HTTP/MCP startup'
