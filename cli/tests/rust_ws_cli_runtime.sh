#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CLI="${FORKLAUNCH_CLI:-${FORKLAUNCH_TEST_CLI:-$REPO_ROOT/cli/target/debug/forklaunch}}"
TSC_BIN="${TSC_BIN:-$REPO_ROOT/framework/node_modules/.bin/tsc}"
mkdir -p output/rust-ws-cli-runtime
cd "$(mktemp -d output/rust-ws-cli-runtime/run.XXXXXX)"
"$CLI" init application realtime-app --path app --language rust -D Realtime -A ForkLaunch -L MIT
"$CLI" init service realtime --path app --language rust -d postgresql -D Realtime
mv app relocated-app
cd relocated-app
mkdir -p .forklaunch/socket-contracts/realtime
cp "$SCRIPT_DIR/fixtures/rust_socket_contract.json" .forklaunch/socket-contracts/realtime/socket-contract.json
"$CLI" sdk generate --path . --target all --from socket-contract
"$CLI" sdk generate --path . --target all --from socket-contract --check
"$CLI" sdk watch --path . --target all --from socket-contract --service realtime --once
python3 "$SCRIPT_DIR/rust_ws_cli_checks.py" "$CLI"
python3 - <<'PYTHON'
from pathlib import Path
import json, os
sdk=Path('sdks/typescript-websocket/realtime')
root=Path(os.environ['FORKLAUNCH_TEST_WS_SDK_DEPENDENCIES'])/'node_modules'
validator=Path(os.environ['FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES'])/'node_modules'
(sdk/'node_modules/@types').mkdir(parents=True)
for name,base in [('ws',validator),('ajv',root),('ajv-formats',root),('ajv-draft-04',root),('@types/ws',validator),('@types/node',validator)]:
    (sdk/'node_modules'/name).symlink_to(base/name,target_is_directory=True)
config=sdk/'tsconfig.json';value=json.loads(config.read_text());value['compilerOptions']['noUnusedLocals']=False
config.write_text(json.dumps(value))
PYTHON
# User compiler choices are retained by regeneration and check.
"$CLI" sdk generate --path . --target all --from socket-contract --check
"$TSC_BIN" --project sdks/typescript-websocket/realtime/tsconfig.json
cargo rustc -p realtime-websocket-sdk --lib -- -D warnings
mkdir -p sdks/rust/realtime-websocket-sdk/examples
cat >> sdks/rust/realtime-websocket-sdk/Cargo.toml <<'TOML'

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
TOML
cat > sdks/rust/realtime-websocket-sdk/examples/roundtrip.rs <<'RUST'
use realtime_websocket_sdk::socket_chat::*;
use forklaunch_ws::transport::{IntoClientRequest, SocketEvent, SocketOptions};
#[tokio::main]
async fn main() {
    let url = std::env::args().nth(1).unwrap();
    assert!(SocketClient::connect(url.as_str(), SocketOptions::default()).await.is_err());
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert("authorization", "Bearer fixture-token".parse().unwrap());
    request.headers_mut().insert("sec-websocket-protocol", "chat.v1".parse().unwrap());
    let (mut client, response) = SocketClient::connect(request, SocketOptions::default()).await.unwrap();
    assert_eq!(response.headers()["sec-websocket-protocol"], "chat.v1");
    assert!(client.send(&SocketOutgoing { message: String::new() }).await.is_err());
    assert!(client.ping(Some(&SocketPing { ts: 0 })).await.is_err());
    client.send(&SocketOutgoing { message: "hello 水 🦀".into() }).await.unwrap();
    match client.recv().await.unwrap().unwrap() { SocketEvent::Message(value) => assert_eq!(value.accepted, "hello 水 🦀"), other => panic!("{other:?}") }
    client.ping(Some(&SocketPing { ts: 42 })).await.unwrap();
    match client.recv().await.unwrap().unwrap() { SocketEvent::Pong(value) => assert_eq!(value.ts, 42), other => panic!("{other:?}") }
    client.close(1000, Some(&SocketClose { reason: "done".into() })).await.unwrap();
    match client.recv().await.unwrap().unwrap() { SocketEvent::Close { code, reason } => { assert_eq!(code, 1000); assert_eq!(reason.reason, "done"); }, other => panic!("{other:?}") }
    assert!(client.recv().await.unwrap().is_none());
    println!("PASS CLI-generated Rust socket client");
}
RUST
cargo build -p realtime-websocket-sdk --example roundtrip
cargo build -p realtime
BINARY_DIR="$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>console.log(JSON.parse(s).target_directory+"/debug"))')"
node "$SCRIPT_DIR/rust_mcp_service_startup.mjs" "$BINARY_DIR/realtime"
node "$SCRIPT_DIR/rust_ws_cli_runtime.mjs" "$PWD" "$BINARY_DIR/examples/roundtrip"
echo 'PASS relocated CLI socket contracts, multi-socket TypeScript/Rust consumers, AsyncAPI and generated HTTP/MCP startup'
