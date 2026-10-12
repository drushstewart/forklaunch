use super::*;
use serde_json::json;
#[tokio::test]
async fn seven_year_anonymization_boundary_dry_run_batches_and_replay() {
    exercise(Arc::new(crate::store::memory::MemoryAuthStore::default())).await;
}
pub(crate) async fn exercise<S: AuthStore>(store: Arc<S>) {
    let now = DateTime::parse_from_rfc3339("2028-02-29T12:34:56.789Z")
        .unwrap()
        .with_timezone(&Utc);
    let cutoff = account_cutoff(now).unwrap();
    seed(store.as_ref(), cutoff).await;
    let service = RetentionService::new(store.clone());
    let dry = service
        .enforce(
            now,
            &RetentionOptions {
                dry_run: true,
                batch_size: 1,
                entities: None,
            },
        )
        .await;
    assert_eq!(dry.anonymized, 2);
    assert_eq!(dry.processed, 2);
    assert_eq!(dry.errors, 0);
    let skipped = service
        .enforce(
            now,
            &RetentionOptions {
                entities: Some(vec!["Jwks".into()]),
                ..RetentionOptions::default()
            },
        )
        .await;
    assert_eq!(skipped.processed, 0);
    assert!(skipped.by_entity.is_empty());
    let result = service
        .enforce(
            now,
            &RetentionOptions {
                batch_size: 1,
                ..RetentionOptions::default()
            },
        )
        .await;
    assert_eq!(result.anonymized, 2);
    assert_eq!(result.deleted, 0);
    assert_eq!(result.errors, 0);
    let mut tx = store.begin(false).await.unwrap();
    for id in ["retention-before", "retention-empty"] {
        let row = tx.get(Model::Account, id).await.unwrap().unwrap();
        for field in ["accessToken", "refreshToken", "idToken", "password"] {
            assert!(row[field].is_null());
        }
        assert_eq!(row["retentionAnonymizedAt"], json!(stamp(now)));
        assert_eq!(row["updatedAt"], json!(stamp(now)));
        assert_eq!(row["scope"], json!("keep-this-scope"));
        assert_eq!(row["issuer"], json!("keep-this-issuer"));
    }
    for id in ["retention-exact", "retention-after", "retention-marked"] {
        assert!(
            tx.get(Model::Account, id).await.unwrap().unwrap()["accessToken"]
                .as_str()
                .is_some()
        );
    }
    tx.commit().await.unwrap();
    assert_eq!(
        service
            .enforce(now, &RetentionOptions::default())
            .await
            .processed,
        0
    );
    assert_eq!(
        service
            .enforce(
                now,
                &RetentionOptions {
                    dry_run: true,
                    ..RetentionOptions::default()
                }
            )
            .await
            .processed,
        0
    );
}

async fn seed<S: AuthStore>(store: &S, cutoff: DateTime<Utc>) {
    let mut tx = store.begin(true).await.unwrap();
    for (id, created, marked, token) in [
        (
            "retention-before",
            cutoff - Duration::milliseconds(1),
            Value::Null,
            json!("native-retention-before-private"),
        ),
        (
            "retention-exact",
            cutoff,
            Value::Null,
            json!("native-retention-exact-private"),
        ),
        (
            "retention-after",
            cutoff + Duration::milliseconds(1),
            Value::Null,
            json!("native-retention-after-private"),
        ),
        (
            "retention-empty",
            cutoff - Duration::days(1),
            Value::Null,
            Value::Null,
        ),
        (
            "retention-marked",
            cutoff - Duration::days(2),
            json!("2027-01-01T00:00:00.000Z"),
            json!("native-retention-marked-private"),
        ),
    ] {
        tx.insert(Model::Account,json!({"id":id,"userId":"retention-owner","providerId":"fixture","accountId":id,"accessToken":token,"refreshToken":token,"idToken":token,"password":token,"createdAt":stamp(created),"updatedAt":stamp(created),"retentionAnonymizedAt":marked,"scope":"keep-this-scope","issuer":"keep-this-issuer"})).await.unwrap();
    }
    tx.commit().await.unwrap();
}
fn stamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
