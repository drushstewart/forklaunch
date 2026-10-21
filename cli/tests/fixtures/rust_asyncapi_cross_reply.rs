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
