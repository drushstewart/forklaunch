use std::time::Duration;

use streams_sdk::{
    SdkError, StreamsClient, StreamsEventsWatchEventsRequest,
    StreamsEventsWatchEventsResponses as Reply,
};
fn input(mode: &str) -> StreamsEventsWatchEventsRequest {
    StreamsEventsWatchEventsRequest {
        params: [("mode".into(), mode.into())].into(),
        ..Default::default()
    }
}
#[tokio::main]
async fn main() {
    tokio::time::timeout(Duration::from_secs(15), verify())
        .await
        .expect("SSE SDK deadline");
    println!(
        "PASS Rust SDK incremental SSE, Unicode, limits, authentication, incomplete EOF, cancellation and transport errors"
    );
}
async fn verify() {
    let base = std::env::args().nth(1).unwrap();
    let client = StreamsClient::new(&base).with_bearer_token("stream-secret");
    assert!(matches!(
        StreamsClient::new(&base)
            .events_watch_events(input("finite"))
            .await,
        Err(SdkError::Contract(_))
    ));
    for mode in ["finite", "multiline"] {
        let Reply::Status200(mut stream) = client.events_watch_events(input(mode)).await.unwrap()
        else {
            panic!("stream missing")
        };
        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.data, serde_json::json!({"label":"水 🦀"}));
        assert_eq!(first.id.as_deref(), Some("first"));
        let second = stream.next().await.unwrap().unwrap();
        assert_eq!(
            second.data,
            if mode == "finite" {
                serde_json::json!(2)
            } else {
                serde_json::json!(false)
            }
        );
        assert_eq!(
            second.id.as_deref(),
            Some(if mode == "finite" { "first" } else { "" })
        );
        assert!(stream.next().await.unwrap().is_none());
    }
    let Reply::Status200(mut stream) = client.events_watch_events(input("slow")).await.unwrap()
    else {
        panic!("stream missing")
    };
    assert!(
        tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .unwrap()
            .is_some()
    );
    stream.close();
    assert!(stream.next().await.unwrap().is_none());
    let Reply::Status200(stream) = client.events_watch_events(input("slow")).await.unwrap() else {
        panic!("stream missing")
    };
    drop(stream);
    let Reply::Status200(mut stream) = client
        .events_watch_events(input("truncated"))
        .await
        .unwrap()
    else {
        panic!("stream missing")
    };
    assert!(stream.next().await.unwrap().is_none());
    let raw = reqwest::Client::new()
        .get(format!("{base}/events/slow"))
        .bearer_auth("stream-secret")
        .send()
        .await
        .unwrap();
    let mut invalid_type = streams_sdk::SdkEventStream::<u8>::from_response(raw, 1048576, "{}").unwrap();
    assert!(matches!(
        invalid_type.next().await,
        Err(SdkError::Contract(_))
    ));
    assert!(invalid_type.next().await.unwrap().is_none());
    assert!(client.clone().with_sse_event_limit(0).is_err());
    let limited = client.clone().with_sse_event_limit(32).unwrap();
    let Reply::Status200(mut stream) = limited
        .events_watch_events(input("oversize"))
        .await
        .unwrap()
    else {
        panic!("stream missing")
    };
    assert!(matches!(stream.next().await, Err(SdkError::Sse(_))));
    assert!(stream.next().await.unwrap().is_none());
    let Reply::Status200(mut late) = client
        .clone()
        .with_sse_event_limit(128)
        .unwrap()
        .events_watch_events(input("late-oversize"))
        .await
        .unwrap()
    else {
        panic!("stream missing")
    };
    assert_eq!(
        late.next().await.unwrap().unwrap().data,
        serde_json::json!({"label":"水 🦀"})
    );
    assert!(matches!(late.next().await, Err(SdkError::Sse(_))));
    assert!(matches!(
        client.events_watch_events(input("wrong")).await,
        Err(SdkError::Contract(_))
    ));
    let Reply::Status200(mut stream) = client.events_watch_events(input("broken")).await.unwrap()
    else {
        panic!("stream missing")
    };
    assert!(stream.next().await.unwrap().is_some());
    assert!(stream.next().await.unwrap().is_some());
    assert!(matches!(stream.next().await, Err(SdkError::Transport(_))));
    assert!(stream.next().await.unwrap().is_none());
}
