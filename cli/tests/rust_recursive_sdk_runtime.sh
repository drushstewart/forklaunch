#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
mkdir -p output/rust-recursive-sdk-runtime
cd "$(mktemp -d output/rust-recursive-sdk-runtime/run.XXXXXX)"
"$CLI" init application trees-app --path app --language rust -D Trees -A ForkLaunch -L MIT
"$CLI" init service trees --path app --language rust -d postgresql -D Trees
mv app relocated-app
cd relocated-app
cp "$SCRIPT_DIR/fixtures/rust_recursive_service.rs" crates/trees/src/recursive_fixture.rs
python3 - <<'PYTHON'
from pathlib import Path
p=Path('crates/trees/Cargo.toml');p.write_text(p.read_text().replace('[dependencies]\n','[dependencies]\nserde_json = "1"\n'))
p=Path('crates/trees/src/lib.rs');s='pub mod recursive_fixture;\n'+p.read_text();s=s.replace('for contract in routes::contracts() {','for contract in routes::contracts().into_iter().chain(recursive_fixture::contracts()) {');p.write_text(s)
p=Path('crates/trees/src/main.rs');s=p.read_text().replace('router(state).serve_with_default_mcp','trees::recursive_fixture::register(router(state)).serve_with_default_mcp');p.write_text(s)
PYTHON
cargo build -p trees
"$CLI" openapi export --path .
if [ "${FORKLAUNCH_TEST_COMPONENT_NAMES:-}" = 1 ]; then
    python3 - <<'PYTHON'
import json,re
from pathlib import Path
document=json.loads(Path('.forklaunch/openapi/trees/openapi.json').read_text())
components=document['components']['schemas']
assert all(re.fullmatch(r'[a-zA-Z0-9._-]+',name) for name in components)
encoded=[name for name in components if name.startswith('_fl_encoded_')]
assert len(encoded)==4, encoded
assert len({document['paths'][path]['get']['responses']['200']['content']['application/json']['schema']['$ref'] for path in ['/trees','/forests','/choices','/mutual']})==4
print('PASS valid and collision-safe component names in the generated OpenAPI document')
PYTHON
fi
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/trees"))')"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY"
for artifact in contract openapi; do
    "$CLI" sdk generate --path . --target all --from "$artifact"
    "$CLI" sdk generate --path . --target all --from "$artifact" --check
    cp "$SCRIPT_DIR/fixtures/rust_recursive_sdk.ts" sdks/typescript/src/recursive_fixture.ts
    "$TSC_BIN" --project sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/sdks/typescript/dist"
    cat >> sdks/rust/trees-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
TOML
    mkdir -p sdks/rust/trees-sdk/examples
    cp "$SCRIPT_DIR/fixtures/rust_recursive_sdk.rs" sdks/rust/trees-sdk/examples/recursive_roundtrip.rs
    cargo build -p trees-sdk --example recursive_roundtrip
    cargo clippy -p trees-sdk --all-targets -- -D warnings
    node "$SCRIPT_DIR/rust_recursive_sdk_runtime.mjs" "$BINARY" "$(dirname "$BINARY")/examples/recursive_roundtrip" "$PWD/sdks/typescript/dist/recursive_fixture.js"
done
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-rust --example recursive_client -- \
    "$PWD/.forklaunch/contracts/trees/contract.json" "$PWD/sdks/rust/trees-sdk/src/lib.rs"
python3 - "$SCRIPT_DIR/fixtures/rust_recursive_sdk.rs" <<'PYTHON'
from pathlib import Path
import sys
source=Path(sys.argv[1]).read_text()
for prefix in ['Trees','Forests','Choices','Mutual']:
    source=source.replace('Trees'+prefix,prefix)
Path('sdks/rust/trees-sdk/examples/recursive_roundtrip.rs').write_text(source)
PYTHON
cargo build -p trees-sdk --example recursive_roundtrip
cargo clippy -p trees-sdk --all-targets -- -D warnings
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-typescript --example recursive_client -- "$PWD/.forklaunch/contracts/trees/contract.json" "$PWD/sdks/typescript/src/index.ts"
"$TSC_BIN" --project sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/sdks/typescript/dist"
node "$SCRIPT_DIR/rust_recursive_sdk_runtime.mjs" "$BINARY" "$(dirname "$BINARY")/examples/recursive_roundtrip" "$PWD/sdks/typescript/dist/recursive_fixture.js"
echo 'PASS relocated generated CLI/both native recursive SDKs, both artifact formats and HTTP/MCP startup'
