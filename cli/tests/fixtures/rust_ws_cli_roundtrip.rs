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
