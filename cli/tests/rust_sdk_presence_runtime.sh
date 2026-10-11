#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
export TSC_BIN
mkdir -p output/rust-sdk-presence-runtime
cd "$(mktemp -d output/rust-sdk-presence-runtime/run.XXXXXX)"
"$CLI" init application presence-app --path app --language rust -D Presence -A ForkLaunch -L MIT
"$CLI" init service presence --path app --language rust -d postgresql -D Presence
mv app relocated-app
cd relocated-app
node "$SCRIPT_DIR/rust_sdk_presence_schema.mjs" "$PWD"
cp "$SCRIPT_DIR/fixtures/rust_sdk_presence_service.rs" crates/presence/src/presence_fixture.rs
python3 - <<'PYTHON'
from pathlib import Path
p=Path('crates/presence/Cargo.toml');p.write_text(p.read_text().replace('[dependencies]\n','[dependencies]\nserde_json = "1"\n'))
p=Path('crates/presence/src/lib.rs');s='pub mod presence_fixture;\n'+p.read_text();s=s.replace('for contract in routes::contracts() {','for contract in routes::contracts().into_iter().chain(presence_fixture::contracts()) {');p.write_text(s)
p=Path('crates/presence/src/main.rs');s=p.read_text().replace('router(state).serve_with_default_mcp','presence::presence_fixture::register(router(state)).serve_with_default_mcp');p.write_text(s)
PYTHON
cargo build -p presence
"$CLI" openapi export --path .
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/presence"))')"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY"
for artifact in contract openapi; do
    "$CLI" sdk generate --path . --target all --from "$artifact"
    cp "$SCRIPT_DIR/fixtures/rust_sdk_presence.ts" sdks/typescript/src/presence_fixture.ts
    "$TSC_BIN" --project sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/sdks/typescript/dist"
    cat >> sdks/rust/presence-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
TOML
    mkdir -p sdks/rust/presence-sdk/examples
    cp "$SCRIPT_DIR/fixtures/rust_sdk_presence.rs" sdks/rust/presence-sdk/examples/presence_roundtrip.rs
    cargo build -p presence-sdk --example presence_roundtrip
    cargo clippy -p presence-sdk --all-targets -- -D warnings
    node "$SCRIPT_DIR/rust_sdk_presence_runtime.mjs" "$BINARY" "$(dirname "$BINARY")/examples/presence_roundtrip" "$PWD/sdks/typescript/dist/presence_fixture.js" "$PWD/crates/presence/src/presence_cases.json"
done
echo 'PASS generated SDK field presence/null parity against actual TypeScript validator, Rust and TypeScript HTTP services and MCP'
