#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
mkdir -p output/rust-asyncapi-import
cd "$(mktemp -d output/rust-asyncapi-import/run.XXXXXX)"
"$CLI" init application import-app --path app --language rust -D Import -A ForkLaunch -L MIT
"$CLI" init service realtime --path app --language rust -d postgresql -D Realtime
mv app relocated-app
cd relocated-app
mkdir -p .forklaunch/asyncapi/realtime
cp "$SCRIPT_DIR/fixtures/rust_asyncapi_socket.json" .forklaunch/asyncapi/realtime/asyncapi.json
if [[ "${FORKLAUNCH_TEST_ASYNCAPI_RESOURCES:-0}" == "1" ]]; then
python3 - <<'PYTHON_RESOURCES'
from pathlib import Path
import json
p=Path('.forklaunch/asyncapi/realtime/asyncapi.json')
document=json.loads(p.read_text())
for name,schema in document['components']['schemas'].items():
    schema['$id']=f'https://schemas.fixture.test/{name}'
p.write_text(json.dumps(document))
PYTHON_RESOURCES
fi
"$CLI" sdk generate --path . --target all --from asyncapi
"$CLI" sdk generate --path . --target all --from asyncapi --check
"$CLI" sdk watch --path . --target all --from asyncapi --service realtime --once
python3 "$SCRIPT_DIR/rust_asyncapi_import_checks.py" "$CLI"
python3 - <<'PYTHON'
from pathlib import Path
import os
sdk=Path('sdks/typescript-websocket/realtime')
root=Path(os.environ['FORKLAUNCH_TEST_WS_SDK_DEPENDENCIES'])/'node_modules'
validator=Path(os.environ['FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES'])/'node_modules'
(sdk/'node_modules/@types').mkdir(parents=True)
for name,base in [('ws',validator),('ajv',root),('ajv-formats',root),('ajv-draft-04',root),('@types/ws',validator),('@types/node',validator)]:
    (sdk/'node_modules'/name).symlink_to(base/name,target_is_directory=True)
PYTHON
"$TSC_BIN" --project sdks/typescript-websocket/realtime/tsconfig.json
cargo rustc -p realtime-websocket-sdk --lib -- -D warnings
mkdir -p sdks/rust/realtime-websocket-sdk/examples
cat >> sdks/rust/realtime-websocket-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
TOML
cat > sdks/rust/realtime-websocket-sdk/examples/import.rs <<'RUST'
use realtime_websocket_sdk::socket_chat::*;
use forklaunch_ws::transport::{SocketEvent, SocketOptions};
#[tokio::main]
async fn main() {
    let url = std::env::args().nth(1).unwrap();
    let (mut client, _) = SocketClient::connect(url.as_str(), SocketOptions::default()).await.unwrap();
    let invalid = serde_json::from_value(serde_json::json!({"kind":"other","count":0})).unwrap();
    assert!(client.send(&invalid).await.is_err());
    for value in [serde_json::json!({"kind":"request","message":"hello 水 🦀"}), serde_json::json!({"kind":"other","count":2})] {
        let message = serde_json::from_value(value).unwrap();
        client.send(&message).await.unwrap();
        match client.recv().await.unwrap().unwrap() {
            SocketEvent::Message(value) => { assert!(value.accepted); assert_eq!(value.kind, "reply"); }
            other => panic!("{other:?}"),
        }
    }
    client.close(1000, None).await.unwrap();
    println!("PASS imported Rust socket client");
}
RUST
cargo build -p realtime-websocket-sdk --example import
cargo build -p realtime
BINARY_DIR="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug"))')"
node "$SCRIPT_DIR/rust_asyncapi_import_runtime.mjs" "$PWD" "$BINARY_DIR/examples/import"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY_DIR/realtime"
python3 "$SCRIPT_DIR/rust_asyncapi_import_checks.py" "$CLI" --rename
cargo build -p stream-events
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY_DIR/stream-events"
python3 "$SCRIPT_DIR/rust_asyncapi_import_checks.py" "$CLI" --delete
echo 'PASS relocated AsyncAPI import, strict SDK consumers, live peers, lifecycle and generated HTTP/MCP startup'
