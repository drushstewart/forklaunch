#![cfg(feature = "postgres")]
use forklaunch_blueprint_core::AuditContext;
use forklaunch_core::persistence::{TenantContext, encryption::FieldEncryptor};
use forklaunch_messaging_base::{new_record, postgres::PostgresSmsStore};
use forklaunch_messaging_interface::{SendSmsDto, SmsStatus, SmsStore};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

#[allow(clippy::too_many_lines)] // End-to-end storage/security lifecycle in one owned tenant fixture.
#[tokio::test]
#[ignore = "requires an isolated FORKLAUNCH_TEST_POSTGRES_URL owner database"]
async fn postgres_sms_encryption_tenants_audit_rotation_and_compliance() {
    let url = std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").unwrap();
    let owner = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    PostgresSmsStore::migrate(&owner).await.unwrap();
    PostgresSmsStore::migrate(&owner).await.unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let role = format!("sms_{suffix}");
    let password = uuid::Uuid::new_v4().to_string();
    // All dynamic SQL identifiers originate from UUID hex, never user input.
    sqlx::raw_sql(&format!("CREATE ROLE {role} LOGIN PASSWORD '{password}'; GRANT USAGE ON SCHEMA fl_messaging TO {role}; GRANT SELECT,INSERT,UPDATE,DELETE ON fl_messaging.sms_records TO {role}; GRANT INSERT ON fl_messaging.pii_read_audit TO {role};")).execute(&owner).await.unwrap();
    let mut runtime_url: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    runtime_url = runtime_url.username(&role).password(&password);
    let runtime = PgPoolOptions::new()
        .max_connections(3)
        .connect_with(runtime_url)
        .await
        .unwrap();
    let tenant_a = TenantContext::new(format!("sms-a-{suffix}")).unwrap();
    let tenant_b = TenantContext::new(format!("sms-b-{suffix}")).unwrap();
    let encryptor = Arc::new(FieldEncryptor::new("test-old-secret-only").unwrap());
    let audit = AuditContext::actor("fixture-actor", format!("fixture-{suffix}"));
    assert_eq!(
        PostgresSmsStore::new(
            owner.clone(),
            tenant_a.clone(),
            Arc::clone(&encryptor),
            audit.clone()
        )
        .await
        .err()
        .unwrap()
        .code,
        "sms_unsafe_database_role"
    );
    let a = PostgresSmsStore::new(
        runtime.clone(),
        tenant_a.clone(),
        Arc::clone(&encryptor),
        audit.clone(),
    )
    .await
    .unwrap();
    let b = PostgresSmsStore::new(
        runtime.clone(),
        tenant_b.clone(),
        Arc::clone(&encryptor),
        audit.clone(),
    )
    .await
    .unwrap();
    let mut record = new_record(SendSmsDto {
        to: "+15551234567".into(),
        body: "secret SMS 💬".into(),
        metadata: Some(serde_json::from_str(r#"{"private":"metadata"}"#).unwrap()),
    });
    record.provider_message_id = Some("SM-shared-provider-id".into());
    record.error = Some("potentially sensitive provider error".into());
    a.insert(record.clone()).await.unwrap();
    b.insert(record.clone()).await.unwrap();
    assert!(a.insert(record.clone()).await.is_err());
    assert_eq!(a.get(&record.id).await.unwrap(), record);
    let mut other = record.clone();
    other.id = uuid::Uuid::new_v4().to_string();
    other.provider_message_id = Some("SM-second".into());
    a.insert(other.clone()).await.unwrap();
    assert_eq!(b.get(&other.id).await.unwrap_err().code, "sms_not_found");
    let (ciphertext,): (String,) = sqlx::query_as(
        "SELECT encrypted_record FROM fl_messaging.sms_records WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_a.id())
    .bind(&record.id)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert!(ciphertext.starts_with("v4:"));
    for plaintext in [
        &record.to,
        &record.body,
        record.error.as_ref().unwrap(),
        "metadata",
    ] {
        assert!(!ciphertext.contains(plaintext));
    }
    assert!(encryptor.decrypt(&ciphertext, tenant_b.id()).is_err());
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM fl_messaging.sms_records")
        .fetch_one(&runtime)
        .await
        .unwrap();
    assert_eq!(count, 0, "unbound pooled connections cannot see tenants");
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM fl_messaging.pii_read_audit WHERE tenant_id=$1 AND record_id=$2 AND actor_id='fixture-actor'").bind(tenant_a.id()).bind(&record.id).fetch_one(&owner).await.unwrap();
    assert_eq!(count, 1);
    let updated = a
        .update_delivery(
            "SM-shared-provider-id",
            SmsStatus::Failed,
            Some("delivery error".into()),
        )
        .await
        .unwrap();
    assert_eq!(updated.status, SmsStatus::Failed);
    let updated = a
        .update_delivery("SM-shared-provider-id", SmsStatus::Delivered, None)
        .await
        .unwrap();
    assert_eq!(updated.error, None);
    assert_eq!(b.get(&record.id).await.unwrap().status, SmsStatus::Queued);
    assert_eq!(
        a.update_delivery("unknown", SmsStatus::Sent, None)
            .await
            .unwrap_err()
            .code,
        "sms_not_found"
    );
    // A same-tenant ciphertext swap must not return another record's plaintext.
    sqlx::query(
        "UPDATE fl_messaging.sms_records SET encrypted_record=$3 WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_a.id())
    .bind(&other.id)
    .bind(&ciphertext)
    .execute(&owner)
    .await
    .unwrap();
    assert!(a.get(&other.id).await.is_err());
    // Audit insertion failures are fail-closed and roll callback changes back.
    sqlx::raw_sql(&format!(
        "REVOKE INSERT ON fl_messaging.pii_read_audit FROM {role}"
    ))
    .execute(&owner)
    .await
    .unwrap();
    assert!(a.get(&record.id).await.is_err());
    assert!(
        a.update_delivery("SM-shared-provider-id", SmsStatus::Undelivered, None)
            .await
            .is_err()
    );
    sqlx::raw_sql(&format!(
        "GRANT INSERT ON fl_messaging.pii_read_audit TO {role}"
    ))
    .execute(&owner)
    .await
    .unwrap();
    assert_eq!(
        a.get(&record.id).await.unwrap().status,
        SmsStatus::Delivered
    );
    sqlx::query("DELETE FROM fl_messaging.sms_records WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_a.id())
        .bind(&other.id)
        .execute(&owner)
        .await
        .unwrap();
    let rotated = Arc::new(
        FieldEncryptor::new("test-new-secret-only")
            .unwrap()
            .with_previous_keys(&["test-old-secret-only".into()])
            .unwrap(),
    );
    let a = PostgresSmsStore::new(runtime.clone(), tenant_a.clone(), rotated, audit)
        .await
        .unwrap();
    assert_eq!(a.export_subject(&record.to).await.unwrap().len(), 1);
    assert!(a.export_subject("different").await.unwrap().is_empty());
    a.update_delivery("SM-shared-provider-id", SmsStatus::Sent, None)
        .await
        .unwrap();
    let (rotated,): (String,) = sqlx::query_as(
        "SELECT encrypted_record FROM fl_messaging.sms_records WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_a.id())
    .bind(&record.id)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_ne!(rotated, ciphertext);
    assert!(encryptor.decrypt(&rotated, tenant_a.id()).is_err());
    let retired = PostgresSmsStore::new(
        runtime.clone(),
        tenant_a.clone(),
        Arc::new(FieldEncryptor::new("test-new-secret-only").unwrap()),
        AuditContext::system("key-retirement-fixture"),
    )
    .await
    .unwrap();
    assert_eq!(
        retired.export_subject(&record.to).await.unwrap().len(),
        1,
        "updated blind indexes survive retirement of the old key"
    );
    assert_eq!(a.erase_subject(&record.to).await.unwrap(), 1);
    assert_eq!(a.erase_subject(&record.to).await.unwrap(), 0);
    assert_eq!(a.get(&record.id).await.unwrap_err().code, "sms_not_found");
    assert_eq!(b.get(&record.id).await.unwrap().body, record.body);
    sqlx::query("DELETE FROM fl_messaging.sms_records WHERE tenant_id=$1")
        .bind(tenant_b.id())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM fl_messaging.pii_read_audit WHERE tenant_id=ANY($1)")
        .bind(vec![tenant_a.id(), tenant_b.id()])
        .execute(&owner)
        .await
        .unwrap();
    runtime.close().await;
    sqlx::raw_sql(&format!("DROP OWNED BY {role}; DROP ROLE {role}"))
        .execute(&owner)
        .await
        .unwrap();
}
