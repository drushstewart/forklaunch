#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
mkdir -p output/rust-versioned-sdk-runtime
cd "$(mktemp -d output/rust-versioned-sdk-runtime/run.XXXXXX)"
"$CLI" init application versions-app --path app --language rust -D Versions -A ForkLaunch -L MIT
"$CLI" init service versions --path app --language rust -d postgresql -D Versions
mv app relocated-app
cd relocated-app
cp "$SCRIPT_DIR/fixtures/rust_versioned_service.rs" crates/versions/src/version_fixture.rs
python3 - <<'PYTHON'
from pathlib import Path
p=Path('crates/versions/Cargo.toml');p.write_text(p.read_text().replace('[dependencies]\n','[dependencies]\nserde_json = "1"\n'))
p=Path('crates/versions/src/lib.rs');s='pub mod version_fixture;\n'+p.read_text();s=s.replace('for contract in routes::contracts() {','for contract in routes::contracts().into_iter().chain(version_fixture::contracts()) {');p.write_text(s)
p=Path('crates/versions/src/main.rs');p.write_text(p.read_text().replace('router(state).serve_with_default_mcp','versions::version_fixture::register(router(state)).serve_with_default_mcp'))
PYTHON
cargo build -p versions
"$CLI" openapi export --path .
python3 - <<'PYTHON'
import json
from pathlib import Path
artifact=json.loads(Path('.forklaunch/contracts/versions/contract.json').read_text())
routes=artifact['routes']
scalar=next(route for route in routes if route['path']=='/versioned-scalar')
assert scalar['options']['bodyParser']['jsonStrict'] is False
assert all('bodyParser' not in route['options'] for route in routes if route['path']=='/versions')
PYTHON
BINARY="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug/versions"))')"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY"
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-typescript --example versioned_client -- "$PWD/.forklaunch/contracts/versions/contract.json" "$PWD/.forklaunch/named-sdk.ts" "$PWD/.forklaunch/named-openapi.json"
for artifact in contract openapi named-openapi; do
    artifact_source="$artifact"
    if [ "$artifact" = named-openapi ]; then
        artifact_source=openapi
        cp .forklaunch/named-openapi.json .forklaunch/openapi/versions/openapi.json
    fi
    "$CLI" sdk generate --path . --target all --from "$artifact_source"
    "$CLI" sdk generate --path . --target all --from "$artifact_source" --check
    cp "$SCRIPT_DIR/fixtures/rust_versioned_sdk.ts" sdks/typescript/src/version_fixture.ts
    "$TSC_BIN" --project sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/sdks/typescript/dist"
    cat >> sdks/rust/versions-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
TOML
    mkdir -p sdks/rust/versions-sdk/examples
    cp "$SCRIPT_DIR/fixtures/rust_versioned_sdk.rs" sdks/rust/versions-sdk/examples/version_roundtrip.rs
    cargo build -p versions-sdk --example version_roundtrip
    cargo clippy -p versions-sdk --all-targets -- -D warnings
    node "$SCRIPT_DIR/rust_versioned_sdk_runtime.mjs" "$BINARY" "$(dirname "$BINARY")/examples/version_roundtrip" "$PWD/sdks/typescript/dist/version_fixture.js"
done
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-rust --example versioned_client -- "$PWD/.forklaunch/contracts/versions/contract.json" "$PWD/sdks/rust/versions-sdk/src/lib.rs"
python3 - "$SCRIPT_DIR/fixtures/rust_versioned_sdk.rs" <<'PYTHON'
from pathlib import Path
import sys
source=Path(sys.argv[1]).read_text().replace('VersionsVersions','').replace('versions_write_version','write_version')
Path('sdks/rust/versions-sdk/examples/version_roundtrip.rs').write_text(source)
PYTHON
cargo build -p versions-sdk --example version_roundtrip
cargo clippy -p versions-sdk --all-targets -- -D warnings
cargo run --manifest-path "$REPO_ROOT/framework-rs/Cargo.toml" -p forklaunch-sdk-typescript --example versioned_client -- "$PWD/.forklaunch/contracts/versions/contract.json" "$PWD/sdks/typescript/src/index.ts"
"$TSC_BIN" --project sdks/typescript/tsconfig.json --noEmit false --outDir "$PWD/sdks/typescript/dist"
node "$SCRIPT_DIR/rust_versioned_sdk_runtime.mjs" "$BINARY" "$(dirname "$BINARY")/examples/version_roundtrip" "$PWD/sdks/typescript/dist/version_fixture.js"
echo 'PASS relocated generated/native versioned SDKs through all three artifact forms and HTTP/MCP startup'
