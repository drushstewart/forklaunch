#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
CROSS_FETCH_PATH="${CROSS_FETCH_PATH:-$(dirname "$TSC_BIN")/../cross-fetch}"
run_sse_clients() {
    FORKLAUNCH_TEST_FETCH_MODULE= node "$SCRIPT_DIR/rust_sse_runtime.mjs" "$BINARY" "$(dirname "$BINARY")/examples/sse_roundtrip" "$PWD/sdks/typescript/dist/index.js"
    FORKLAUNCH_TEST_FETCH_MODULE="$CROSS_FETCH_PATH" node "$SCRIPT_DIR/rust_sse_runtime.mjs" "$BINARY" "$(dirname "$BINARY")/examples/sse_roundtrip" "$PWD/sdks/typescript/dist/index.js"
}
mkdir -p output/rust-sse-runtime
cd "$(mktemp -d output/rust-sse-runtime/run.XXXXXX)"
"$CLI" init application streams-app --path app --language rust -D Streams -A ForkLaunch -L MIT
"$CLI" init service streams --path app --language rust -d postgresql -D Streams
mv app relocated-app
cd relocated-app
cp "$SCRIPT_DIR/fixtures/rust_sse_service.rs" crates/streams/src/sse_fixture.rs
python3 - <<'PYTHON'
from pathlib import Path
p=Path('crates/streams/Cargo.toml')
s=p.read_text().replace('[dependencies]\n','[dependencies]\nserde_json = "1"\nfutures-util = "0.3"\nforklaunch-auth = { path = "../../.forklaunch/framework-rs/crates/forklaunch-auth" }\n')
s=s.replace('features = ["mcp"]', 'features = ["mcp", "sse"]')
p.write_text(s)
p=Path('crates/streams/src/lib.rs');s=p.read_text();s='pub mod sse_fixture;\n'+s
s=s.replace('for contract in routes::contracts() {','for contract in routes::contracts().into_iter().chain(sse_fixture::contracts()) {')
p.write_text(s)
p=Path('crates/streams/src/main.rs');s=p.read_text().replace('router(state).serve_with_default_mcp','streams::sse_fixture::register(router(state)).serve_with_default_mcp');p.write_text(s)
PYTHON
cargo build -p streams
"$CLI" openapi export --path .
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/streams"))')"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY"
for artifact in contract openapi; do
    "$CLI" sdk generate --path . --target all --from "$artifact"
    "$TSC_BIN" --project sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/sdks/typescript/dist"
    cat >> sdks/rust/streams-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
TOML
    mkdir -p sdks/rust/streams-sdk/examples
    cp "$SCRIPT_DIR/fixtures/rust_sse_sdk.rs" sdks/rust/streams-sdk/examples/sse_roundtrip.rs
    cargo build -p streams-sdk --example sse_roundtrip
    cargo clippy -p streams-sdk --all-targets -- -D warnings
    run_sse_clients
done
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-rust --example stream_client -- "$PWD/sdks/rust/streams-sdk/src/lib.rs"
python3 - "$SCRIPT_DIR/fixtures/rust_sse_sdk.rs" <<'PYTHON'
from pathlib import Path
import sys
Path('sdks/rust/streams-sdk/examples/sse_roundtrip.rs').write_text(Path(sys.argv[1]).read_text().replace('StreamsEvents','Events'))
PYTHON
cargo build -p streams-sdk --example sse_roundtrip
cargo clippy -p streams-sdk --all-targets -- -D warnings
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-typescript --example stream_client -- "$PWD/sdks/typescript/src/index.ts"
"$TSC_BIN" --project sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/sdks/typescript/dist"
run_sse_clients
# Also exercise the generator's imported-fetch option without overriding config.fetch.
mkdir -p sdks/typescript/node_modules
ln -s "$CROSS_FETCH_PATH" sdks/typescript/node_modules/cross-fetch
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-typescript --example stream_client -- "$PWD/sdks/typescript/src/index.ts" --import-fetch
"$TSC_BIN" --project sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/sdks/typescript/dist"
FORKLAUNCH_TEST_FETCH_MODULE= node "$SCRIPT_DIR/rust_sse_runtime.mjs" "$BINARY" "$(dirname "$BINARY")/examples/sse_roundtrip" "$PWD/sdks/typescript/dist/index.js"
echo 'PASS relocated generated SSE service, CLI/both native clients and both artifact formats'
