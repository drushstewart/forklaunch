#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
mkdir -p output/rust-asyncapi-cross-channel
cd "$(mktemp -d output/rust-asyncapi-cross-channel/run.XXXXXX)"
"$CLI" init application reply-app --path app --language rust -D Replies -A ForkLaunch -L MIT
"$CLI" init service realtime --path app --language rust -d postgresql -D Realtime
mv app relocated-app
cd relocated-app
mkdir -p .forklaunch/asyncapi/realtime
python3 - "$SCRIPT_DIR/fixtures/rust_asyncapi_socket.json" <<'PY'
from pathlib import Path
import json,sys
d=json.loads(Path(sys.argv[1]).read_text())
reply=d['channels']['chat']['messages'].pop('reply')
d['channels']['responses']={'address':'/responses','messages':{'reply':reply}}
d['components']['replies']['response']={
 'channel':{'$ref':'#/channels/responses'},
 'messages':[{'$ref':'#/channels/responses/messages/reply'}]}
Path('.forklaunch/asyncapi/realtime/asyncapi.json').write_text(json.dumps(d))
PY
"$CLI" sdk generate --path . --target all --from asyncapi
"$CLI" sdk generate --path . --target all --from asyncapi --check
"$CLI" sdk watch --path . --target all --from asyncapi --service realtime --once
python3 - <<'PY'
from pathlib import Path
import os
sdk=Path('sdks/typescript-websocket/realtime')
root=Path(os.environ['FORKLAUNCH_TEST_WS_SDK_DEPENDENCIES'])/'node_modules'
validator=Path(os.environ['FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES'])/'node_modules'
(sdk/'node_modules/@types').mkdir(parents=True)
for name,base in [('ws',validator),('ajv',root),('ajv-formats',root),('ajv-draft-04',root),('@types/ws',validator),('@types/node',validator)]:
 (sdk/'node_modules'/name).symlink_to(base/name,target_is_directory=True)
PY
"$TSC_BIN" --project sdks/typescript-websocket/realtime/tsconfig.json
cat >> sdks/rust/realtime-websocket-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
TOML
mkdir -p sdks/rust/realtime-websocket-sdk/examples
cat > sdks/rust/realtime-websocket-sdk/examples/cross_reply.rs <<'RUST'
use realtime_websocket_sdk::{socket_chat, socket_responses};
use forklaunch_ws::transport::{SocketEvent, SocketOptions};
#[tokio::main]
async fn main() {
    let base = std::env::args().nth(1).unwrap();
    let (mut responses, _) = socket_responses::SocketClient::connect(
        &format!("{base}/responses"), SocketOptions::default()).await.unwrap();
    let (mut requests, _) = socket_chat::SocketClient::connect(
        &format!("{base}/chat"), SocketOptions::default()).await.unwrap();
    let invalid = serde_json::from_value(serde_json::json!({"kind":"other","count":0})).unwrap();
    assert!(requests.send(&invalid).await.is_err());
    // The reply socket has no permitted client-to-server messages.
    let forbidden = serde_json::json!({"kind":"reply","accepted":true});
    assert!(responses.send(&forbidden).await.is_err());
    for value in [serde_json::json!({"kind":"request","message":"hello 水 🦀"}), serde_json::json!({"kind":"other","count":2})] {
        requests.send(&serde_json::from_value(value).unwrap()).await.unwrap();
        match responses.recv().await.unwrap().unwrap() {
            SocketEvent::Message(value) => { assert!(value.accepted); assert_eq!(value.kind, "reply"); }
            other => panic!("{other:?}"),
        }
    }
    requests.close(1000, None).await.unwrap();
    responses.close(1000, None).await.unwrap();
    println!("PASS imported Rust cross-channel reply client");
}
RUST
cargo clippy -p realtime-websocket-sdk --all-targets -- -D warnings
cargo build -p realtime-websocket-sdk --example cross_reply
cargo build -p realtime
BINARY_DIR="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug"))')"
node "$SCRIPT_DIR/rust_asyncapi_cross_channel_runtime.mjs" "$PWD" "$BINARY_DIR/examples/cross_reply"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY_DIR/realtime"
echo 'PASS relocated fixed cross-channel replies, strict SDK consumers, independent live sockets and HTTP/MCP startup'
