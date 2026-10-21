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
