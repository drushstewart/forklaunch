use typed_request_proof::*;

#[test]
fn typed_bodies_serialize_once_and_raw_is_explicit() {
    let client = Client::new("http://localhost");
    let body: CreateRequestBody = serde_json::from_value(serde_json::json!({
        "name":"quoted\" 水", "nested":{"enabled":true}, "note":null, "amount":1.25
    }))
    .unwrap();
    let typed = client
        .create_request(CreateRequest {
            body: Some(body),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(typed.content_type, "application/json");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(typed.body.as_deref().unwrap()).unwrap(),
        serde_json::json!({"name":"quoted\" 水","nested":{"enabled":true},"note":null,"amount":1.25})
    );
    assert!(
        client
            .create_request(CreateRequest::default())
            .unwrap()
            .body
            .is_none()
    );
    assert_eq!(
        client
            .list_request(ListRequest {
                body: Some(vec![1, 2]),
                ..Default::default()
            })
            .unwrap()
            .body
            .as_deref(),
        Some("[1,2]")
    );
    assert_eq!(
        client
            .text_request(TextRequest {
                body: Some("x\"y".into()),
                ..Default::default()
            })
            .unwrap()
            .body
            .as_deref(),
        Some("\"x\\\"y\"")
    );
    assert_eq!(
        client
            .null_request(NullRequest {
                body: Some(()),
                ..Default::default()
            })
            .unwrap()
            .body
            .as_deref(),
        Some("null")
    );
    assert!(
        client
            .null_request(NullRequest::default())
            .unwrap()
            .body
            .is_none()
    );
    let raw = client.create_raw_request(CreateRequest::<String> {
        body: Some("{ deliberate malformed JSON".into()),
        ..Default::default()
    });
    assert_eq!(raw.body.as_deref(), Some("{ deliberate malformed JSON"));
    assert!(
        !format!(
            "{:?}",
            CreateRequest {
                body: Some(
                    serde_json::from_value::<CreateRequestBody>(
                        serde_json::json!({"name":"secret","nested":{"enabled":true},"note":null})
                    )
                    .unwrap()
                ),
                ..Default::default()
            }
        )
        .contains("secret")
    );
}

#[tokio::test]
async fn typed_transport_and_auth_hook_see_serialized_json() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let n = socket.read(&mut buffer).unwrap();
            assert_ne!(n, 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    assert!(headers.contains("content-type: application/json"));
                    assert!(headers.contains("x-proof: serialized"));
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(&bytes[end + 4..]).unwrap(),
                        serde_json::json!({"wire":[true,null,"水"]})
                    );
                    break;
                }
            }
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4\r\nConnection: close\r\n\r\n\"ok\"").unwrap();
    });
    let client = Client::new(format!("http://{address}")).with_auth_hook(|request| {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(request.body.as_ref().unwrap()).unwrap(),
            serde_json::json!({"wire":[true,null,"水"]})
        );
        request
            .headers
            .insert("x-proof".into(), "serialized".into());
    });
    let DynamicResponses::Status200(response) = client
        .dynamic(DynamicRequest {
            body: Some(serde_json::json!({"wire":[true,null,"水"]})),
            ..Default::default()
        })
        .await
        .unwrap()
    else {
        panic!("unexpected response")
    };
    assert_eq!(response, "ok");
    server.join().unwrap();
}
