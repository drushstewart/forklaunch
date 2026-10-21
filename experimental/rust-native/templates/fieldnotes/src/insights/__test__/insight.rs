use super::*;
use uuid::Uuid;
fn sealed(body: &str) -> (EnvelopeKey, SealedNoteJob) {
    let key = EnvelopeKey::new([31; 32]);
    let job = NoteJob {
        operation_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        note_id: Uuid::new_v4(),
        revision: 2,
        body: body.into(),
    };
    let payload = key
        .seal(
            job.workspace_id,
            job.operation_id,
            PURPOSE,
            &serde_json::to_vec(&serde_json::json!({"payload": &job})).unwrap(),
        )
        .unwrap();
    (
        key,
        SealedNoteJob {
            operation_id: job.operation_id,
            workspace_id: job.workspace_id,
            note_id: job.note_id,
            revision: job.revision,
            payload,
        },
    )
}
#[test]
fn bounded_unicode_excerpt_and_no_plaintext_wire() {
    let text = "🌱 hello\n世界 ".repeat(80);
    let (key, job) = sealed(&text);
    let result = analyze(&key, &job).unwrap();
    assert_eq!(result.excerpt.chars().count(), 160);
    assert_eq!(result.word_count, 240);
    assert!(!serde_json::to_string(&job).unwrap().contains("hello"));
}
#[test]
fn outer_identity_key_and_ciphertext_are_bound() {
    let (key, job) = sealed("secret canary");
    assert!(analyze(&EnvelopeKey::new([32; 32]), &job).is_err());
    for field in 0..5 {
        let mut wrong = job.clone();
        match field {
            0 => wrong.workspace_id = Uuid::new_v4(),
            1 => wrong.operation_id = Uuid::new_v4(),
            2 => wrong.note_id = Uuid::new_v4(),
            3 => wrong.revision += 1,
            _ => wrong.payload.push('!'),
        };
        assert!(analyze(&key, &wrong).is_err());
    }
    assert_eq!(analyze(&key, &job).unwrap().word_count, 2);
}

#[test]
fn version_two_schema_requires_wrapped_payload_and_rejects_legacy_jobs() {
    let (key, sealed) = sealed("schema canary");
    let traced = worker::decode_job::<fieldnotes_contracts::events::insight::TracedNoteJob>(
        key.as_keyring(),
        sealed.context(),
        PURPOSE,
        &sealed.payload,
    )
    .unwrap();
    let schema = framework_core::contract::schema_value_for::<
        fieldnotes_contracts::events::insight::TracedNoteJob,
    >();
    let validator = framework_core::contract::schema_validation::compile_checked(&schema).unwrap();
    let wrapped = serde_json::to_value(&*traced).unwrap();
    assert!(validator(&wrapped).is_ok());
    assert!(validator(&wrapped["payload"]).is_err());
    let legacy = key
        .seal(
            sealed.workspace_id,
            sealed.operation_id,
            "note-insight-job-v1",
            &serde_json::to_vec(&wrapped["payload"]).unwrap(),
        )
        .unwrap();
    let legacy = SealedNoteJob {
        payload: legacy,
        ..sealed
    };
    assert!(analyze(&key, &legacy).is_err());
}

#[test]
fn unicode_body_limit_matches_http_schema() {
    let (key, job) = sealed(&"🌱".repeat(65_536));
    assert!(analyze(&key, &job).is_ok());
    let (key, job) = sealed(&"🌱".repeat(65_537));
    assert!(analyze(&key, &job).is_err());
}
