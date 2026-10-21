struct TestAudit;
impl worker::JobAccessAudit for TestAudit {
    fn record<'a>(
        &'a self,
        _: worker::JobAccess<'a>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), worker::ProcessorError>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}
use super::*;
use crate::{NoteJob, PURPOSE};
use fieldnotes_security::EnvelopeKey;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

#[tokio::test]
async fn queue_boundary_keeps_invalid_envelopes_permanent_and_private() {
    let state = State {
        audit: Arc::new(TestAudit),
        key: Arc::new(EnvelopeKey::new([43; 32])),
        api: fieldnotes_client::NotesClient::new("http://127.0.0.1:1"),
        secret: Arc::new(zeroize::Zeroizing::new("private-token".into())),
    };
    let job = SealedNoteJob {
        operation_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        note_id: Uuid::new_v4(),
        revision: 1,
        payload: "private-invalid-payload".into(),
    };
    let error = process_job(job, Data::new(state)).await.unwrap_err();
    assert!(
        error
            .downcast_ref::<apalis_core::error::AbortError>()
            .is_some()
    );
    assert_eq!(error.to_string(), "AbortError: worker payload rejected");
}

async fn server(
    response: serde_json::Value,
    workspace: Uuid,
    note: Uuid,
    operation: Uuid,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut wire = Vec::new();
        let mut buf = [0u8; 4096];
        let (header_end, length) = loop {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            wire.extend_from_slice(&buf[..n]);
            assert!(wire.len() < 16384);
            if let Some(index) = wire.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&wire[..index]).unwrap();
                assert!(headers.starts_with(&format!(
                    "POST /internal/workspaces/{workspace}/notes/{note}/insight HTTP/1.1\r\n"
                )));
                assert!(
                    headers
                        .to_ascii_lowercase()
                        .contains("authorization: hmac keyid=worker")
                );
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                break (index + 4, length);
            }
        };
        while wire.len() < header_end + length {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            wire.extend_from_slice(&buf[..n]);
        }
        let body: serde_json::Value =
            serde_json::from_slice(&wire[header_end..header_end + length]).unwrap();
        assert_eq!(body["operationId"], operation.to_string());
        assert_eq!(body["revision"], 7);
        assert!(!std::str::from_utf8(&wire).unwrap().contains("hello"));
        assert!(
            !std::str::from_utf8(&wire)
                .unwrap()
                .contains("local-worker-secret")
        );
        let headers = std::str::from_utf8(&wire[..header_end]).unwrap();
        let authorization = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                    .map(|(_, value)| value.trim())
            })
            .unwrap();
        let security = fieldnotes_security::SecurityService::new(
            "http://127.0.0.1:1".into(),
            "local-worker-secret-at-least-32-bytes".into(),
        )
        .unwrap();
        let path = format!("/internal/workspaces/{workspace}/notes/{note}/insight");
        security
            .authenticate_signed_internal(
                authorization,
                &path,
                &wire[header_end..header_end + length],
                |_, _| async { Ok::<_, ()>(true) },
            )
            .await
            .unwrap();
        let key = EnvelopeKey::new([43; 32]);
        let decoded = worker::decode_job::<fieldnotes_contracts::events::insight::InsightEvent>(
            key.as_keyring(),
            worker::JobContext {
                tenant_id: workspace,
                resource_id: note,
                operation_id: operation,
                revision: 7,
            },
            fieldnotes_contracts::events::insight::RESULT_PURPOSE,
            body["encryptedPayload"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(decoded.excerpt, "hello 🌱 world");
        assert_eq!(decoded.word_count, 3);
        let body = serde_json::to_vec(&response).unwrap();
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(&body).await.unwrap();
    });
    (url, task)
}

#[tokio::test]
async fn generated_client_typed_publication_and_terminal_statuses() {
    for status in ["applied", "duplicate", "stale"] {
        publication(status, false, false).await;
    }
    publication("applied", true, false).await;
    publication("applied", false, true).await;
}
async fn publication(status: &str, wrong_operation: bool, wrong_revision: bool) {
    let key = Arc::new(EnvelopeKey::new([43; 32]));
    let job = NoteJob {
        operation_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        note_id: Uuid::new_v4(),
        revision: 7,
        body: "hello 🌱 world".into(),
    };
    let response = serde_json::json!({"operationId":if wrong_operation {Uuid::new_v4()} else {job.operation_id},"revision":if wrong_revision {8} else {7},"status":status});
    let (url, server) = server(response, job.workspace_id, job.note_id, job.operation_id).await;
    let payload = key
        .seal(
            job.workspace_id,
            job.operation_id,
            PURPOSE,
            &serde_json::to_vec(&serde_json::json!({"payload": &job})).unwrap(),
        )
        .unwrap();
    let sealed = SealedNoteJob {
        operation_id: job.operation_id,
        workspace_id: job.workspace_id,
        note_id: job.note_id,
        revision: job.revision,
        payload,
    };
    let state = State {
        audit: Arc::new(TestAudit),
        key,
        api: fieldnotes_client::NotesClient::new(url),
        secret: Arc::new(zeroize::Zeroizing::new(
            "local-worker-secret-at-least-32-bytes".into(),
        )),
    };
    let outcome = tokio::time::timeout(Duration::from_secs(3), process(sealed, Data::new(state)))
        .await
        .unwrap();
    assert_eq!(outcome.is_ok(), !wrong_operation && !wrong_revision);
    if let Err(error) = outcome {
        assert_eq!(error.to_string(), "insight publication unavailable");
    }
    server.await.unwrap();
}

#[tokio::test]
async fn oversized_authenticated_payload_remains_permanent_after_trace_adoption() {
    let key = Arc::new(EnvelopeKey::new([43; 32]));
    let payload = worker::TracedJob::new(NoteJob {
        operation_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        note_id: Uuid::new_v4(),
        revision: 1,
        body: "🌱".repeat(65_537),
    });
    let sealed = SealedNoteJob {
        operation_id: payload.operation_id,
        workspace_id: payload.workspace_id,
        note_id: payload.note_id,
        revision: payload.revision,
        payload: worker::seal_job(key.as_keyring(), PURPOSE, &payload).unwrap(),
    };
    let state = State {
        key,
        audit: Arc::new(TestAudit),
        api: fieldnotes_client::NotesClient::new("http://127.0.0.1:1"),
        secret: Arc::new(zeroize::Zeroizing::new("unused-secret".into())),
    };
    let error = process_job(sealed, Data::new(state)).await.unwrap_err();
    assert!(
        error
            .downcast_ref::<apalis_core::error::AbortError>()
            .is_some()
    );
    assert_eq!(error.to_string(), "AbortError: worker payload rejected");
}
