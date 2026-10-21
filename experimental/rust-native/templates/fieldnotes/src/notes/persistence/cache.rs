//! Disposable encrypted single-note cache. PostgreSQL selects the version; Redis
//! cannot select a tenant, revive a deleted row, or overwrite a newer version.
use super::compliance::{ProtectedPayload, reveal_payload};
use crate::{NoteRecord, NotesRepository};
use fieldnotes_security::{EnvelopeKey, SecurityError, VerifiedTenant};
use redis_cache::{RedisTtlCacheConfig, TtlCache, client::RedisTtlCache};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use uuid::Uuid;
use zeroize::Zeroizing;

pub(crate) struct NoteCache {
    url: Arc<Zeroizing<String>>,
    budget: Arc<Semaphore>,
}
/// Identity and version selected by PostgreSQL before consulting Redis.
struct ExpectedEntry {
    tenant: Uuid,
    id: Uuid,
    version: i64,
    revision: i64,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    version: i64,
    note: NoteRecord,
}
impl NoteCache {
    fn new(url: String) -> Result<Self, SecurityError> {
        let parsed = reqwest::Url::parse(&url).map_err(|_| SecurityError::InvalidInput)?;
        if !matches!(parsed.scheme(), "redis" | "rediss") || parsed.host_str().is_none() {
            return Err(SecurityError::InvalidInput);
        }
        RedisTtlCacheConfig::new(&url, Duration::from_secs(60))
            .validate()
            .map_err(|_| SecurityError::InvalidInput)?;
        Ok(Self {
            url: Arc::new(Zeroizing::new(url)),
            budget: Arc::new(Semaphore::new(8)),
        })
    }
    fn key(tenant: Uuid, id: Uuid, version: i64) -> String {
        format!("fieldnotes:note:{tenant}:{id}:{version}")
    }
    fn purpose(version: i64) -> String {
        format!("note-cache-v{version}")
    }
    fn encode(key: &EnvelopeKey, version: i64, note: &NoteRecord) -> Option<Vec<u8>> {
        let bytes = Zeroizing::new(
            serde_json::to_vec(&serde_json::json!({"version":version,"note":note})).ok()?,
        );
        key.seal(note.workspace_id, note.id, &Self::purpose(version), &bytes)
            .ok()
            .map(String::into_bytes)
    }
    async fn decode(
        key: &EnvelopeKey,
        expected: ExpectedEntry,
        bytes: &[u8],
        actor: &str,
        audit: &dyn framework_core::persistence::protected::AsyncComplianceAudit,
    ) -> Option<NoteRecord> {
        let ExpectedEntry {
            tenant,
            id,
            version,
            revision,
        } = expected;
        let text = std::str::from_utf8(bytes).ok()?;
        let plaintext = reveal_payload(
            key,
            tenant,
            id,
            ProtectedPayload::NoteCache { version },
            text,
            actor,
            audit,
        )
        .await
        .ok()?;
        let entry: Entry = serde_json::from_slice(&plaintext).ok()?;
        (entry.version == version
            && entry.note.id == id
            && entry.note.workspace_id == tenant
            && entry.note.revision == revision)
            .then_some(entry.note)
    }
    async fn raw(&self, key: String, value: Option<Vec<u8>>) -> Option<Vec<u8>> {
        // Detached blocking socket work is bounded even when the caller times out.
        let permit = self.budget.clone().try_acquire_owned().ok()?;
        let url = self.url.clone();
        tokio::time::timeout(
            Duration::from_millis(200),
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let mut cache = RedisTtlCache::connect(RedisTtlCacheConfig::new(
                    url.as_str(),
                    Duration::from_secs(60),
                ))
                .ok()?;
                if let Some(value) = value {
                    cache.set(key, value, None).ok()?;
                    None
                } else {
                    cache.get(&key).ok().flatten()
                }
            }),
        )
        .await
        .ok()?
        .ok()
        .flatten()
    }
}
impl NotesRepository {
    /// Invalid configuration is an error. Redis outages are cache misses, never authority.
    pub fn with_read_cache_from_env(mut self) -> Result<Self, SecurityError> {
        if let Ok(url) = std::env::var("REDIS_URL") {
            self.cache = Some(NoteCache::new(url)?);
        }
        Ok(self)
    }
    pub(crate) async fn get_with_cache(
        &self,
        scope: &VerifiedTenant,
        id: Uuid,
    ) -> Result<NoteRecord, SecurityError> {
        let mut tx = self.transaction(scope.tenant_id()).await?;
        if self.cache.is_none() {
            let row = sqlx::query("SELECT * FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2")
                .bind(scope.tenant_id())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(SecurityError::NotFound)?;
            let note = self.record(scope, &row).await?;
            tx.commit().await?;
            return Ok(note);
        }
        let current = sqlx::query(
            "SELECT cache_version,revision FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2",
        )
        .bind(scope.tenant_id())
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(SecurityError::NotFound)?;
        let version: i64 = current.try_get("cache_version")?;
        let revision: i64 = current.try_get("revision")?;
        let audit = super::compliance::AccessAudit::new(&self.audit_pool, scope);
        if let Some(cache) = &self.cache
            && let Some(bytes) = cache
                .raw(NoteCache::key(scope.tenant_id(), id, version), None)
                .await
            && let Some(note) = NoteCache::decode(
                &self.key,
                ExpectedEntry {
                    tenant: scope.tenant_id(),
                    id,
                    version,
                    revision,
                },
                &bytes,
                audit.actor(),
                &audit,
            )
            .await
        {
            let still_current:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2 AND cache_version=$3)").bind(scope.tenant_id()).bind(id).bind(version).fetch_one(&mut *tx).await?;
            if still_current {
                tx.commit().await?;
                return Ok(note);
            }
        }
        let row = sqlx::query("SELECT * FROM fieldnotes_notes WHERE workspace_id=$1 AND id=$2")
            .bind(scope.tenant_id())
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(SecurityError::NotFound)?;
        let version: i64 = row.try_get("cache_version")?;
        let note = self.record(scope, &row).await?;
        tx.commit().await?;
        if let Some(cache) = &self.cache
            && let Some(bytes) = NoteCache::encode(&self.key, version, &note)
        {
            cache
                .raw(NoteCache::key(scope.tenant_id(), id, version), Some(bytes))
                .await;
        }
        Ok(note)
    }
}

#[cfg(test)]
pub(super) mod tests {
    pub(crate) async fn verified_scope(tenant: Uuid) -> VerifiedTenant {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0, "request ended before its body");
                request.extend_from_slice(&chunk[..count]);
                assert!(request.len() <= 8192, "bounded auth test request");
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&request[..end]).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body =
                serde_json::json!({"userId":"cache-proof","workspaceId":tenant,"role":"editor"})
                    .to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        let auth = fieldnotes_security::SecurityService::new(
            format!("http://{address}"),
            "test-secret-at-least-thirty-two-bytes".into(),
        )
        .unwrap();
        let scope = auth.authenticate("test-session", tenant).await.unwrap();
        server.await.unwrap();
        scope
    }

    use super::*;
    fn note() -> NoteRecord {
        NoteRecord {
            id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            title: "secret title".into(),
            body: "secret body canary".into(),
            revision: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            insight_excerpt: None,
            insight_word_count: None,
        }
    }
    #[tokio::test]
    async fn ciphertext_is_bound_to_tenant_resource_and_cache_generation() {
        let key = EnvelopeKey::new([71; 32]);
        let note = note();
        let bytes = NoteCache::encode(&key, 2, &note).unwrap();
        assert!(!std::str::from_utf8(&bytes).unwrap().contains("secret"));
        assert!(
            NoteCache::decode(
                &key,
                ExpectedEntry {
                    tenant: note.workspace_id,
                    id: note.id,
                    version: 2,
                    revision: 1
                },
                &bytes,
                "test-user",
                &super::super::compliance::TestAudit
            )
            .await
            .is_some()
        );
        assert!(
            NoteCache::decode(
                &key,
                ExpectedEntry {
                    tenant: Uuid::new_v4(),
                    id: note.id,
                    version: 2,
                    revision: 1
                },
                &bytes,
                "test-user",
                &super::super::compliance::TestAudit
            )
            .await
            .is_none()
        );
        assert!(
            NoteCache::decode(
                &key,
                ExpectedEntry {
                    tenant: note.workspace_id,
                    id: Uuid::new_v4(),
                    version: 2,
                    revision: 1
                },
                &bytes,
                "test-user",
                &super::super::compliance::TestAudit
            )
            .await
            .is_none()
        );
        assert!(
            NoteCache::decode(
                &key,
                ExpectedEntry {
                    tenant: note.workspace_id,
                    id: note.id,
                    version: 3,
                    revision: 1
                },
                &bytes,
                "test-user",
                &super::super::compliance::TestAudit
            )
            .await
            .is_none()
        );
        assert!(
            NoteCache::decode(
                &key,
                ExpectedEntry {
                    tenant: note.workspace_id,
                    id: note.id,
                    version: 2,
                    revision: 2
                },
                &bytes,
                "test-user",
                &super::super::compliance::TestAudit
            )
            .await
            .is_none()
        );
        assert_ne!(
            NoteCache::key(note.workspace_id, note.id, 2),
            NoteCache::key(note.workspace_id, note.id, 3)
        );
    }
    #[tokio::test]
    #[ignore = "requires owned local Redis"]
    async fn redis_contains_ciphertext_and_late_old_fill_cannot_replace_current_key() {
        let url = std::env::var("FIELDNOTES_TEST_REDIS_URL").expect("owned local Redis");
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert!(matches!(
            parsed.host_str(),
            Some("127.0.0.1" | "localhost" | "redis")
        ));
        let cache = NoteCache::new(url.clone()).unwrap();
        let key = EnvelopeKey::new([72; 32]);
        let mut note = note();
        let old = NoteCache::encode(&key, 1, &note).unwrap();
        let old_key = NoteCache::key(note.workspace_id, note.id, 1);
        cache.raw(old_key.clone(), Some(old.clone())).await;
        let raw = cache.raw(old_key.clone(), None).await.unwrap();
        assert!(raw.starts_with(b"v1."));
        assert!(!std::str::from_utf8(&raw).unwrap().contains("canary"));
        note.body = "new content".into();
        note.revision = 2;
        let current_key = NoteCache::key(note.workspace_id, note.id, 2);
        cache
            .raw(
                current_key.clone(),
                Some(NoteCache::encode(&key, 2, &note).unwrap()),
            )
            .await;
        // A slow request finishing the old snapshot writes only its old generation.
        cache.raw(old_key.clone(), Some(old.clone())).await;
        let current = cache.raw(current_key.clone(), None).await.unwrap();
        assert_eq!(
            NoteCache::decode(
                &key,
                ExpectedEntry {
                    tenant: note.workspace_id,
                    id: note.id,
                    version: 2,
                    revision: 2
                },
                &current,
                "test-user",
                &super::super::compliance::TestAudit
            )
            .await
            .unwrap()
            .body,
            "new content"
        );
        let other = Uuid::new_v4();
        let wrong_key = NoteCache::key(other, note.id, 1);
        cache.raw(wrong_key.clone(), Some(old)).await;
        assert!(
            NoteCache::decode(
                &key,
                ExpectedEntry {
                    tenant: other,
                    id: note.id,
                    version: 1,
                    revision: 1
                },
                &cache.raw(wrong_key.clone(), None).await.unwrap(),
                "test-user",
                &super::super::compliance::TestAudit
            )
            .await
            .is_none()
        );
        tokio::task::spawn_blocking(move || {
            let mut backend =
                RedisTtlCache::connect(RedisTtlCacheConfig::new(url, Duration::from_secs(60)))
                    .unwrap();
            for key in [old_key, current_key, wrong_key] {
                backend.delete(&key).unwrap();
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    #[ignore = "requires owned local fieldnotes_cache_proof database and Redis"]
    async fn current_database_generation_controls_update_insight_delete_and_tenant_reads() {
        let db = std::env::var("FIELDNOTES_TEST_CACHE_DATABASE_URL").expect("owned local database");
        let options: sqlx::postgres::PgConnectOptions = db.parse().unwrap();
        assert!(matches!(options.get_host(), "127.0.0.1" | "localhost"));
        assert_eq!(options.get_database(), Some("fieldnotes_cache_proof"));
        let owner = sqlx::PgPool::connect(&db).await.unwrap();
        sqlx::raw_sql("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='fieldnotes_api') THEN CREATE ROLE fieldnotes_api NOLOGIN NOSUPERUSER NOBYPASSRLS; END IF; IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='fieldnotes_worker') THEN CREATE ROLE fieldnotes_worker NOLOGIN NOSUPERUSER NOBYPASSRLS; END IF; END $$;").execute(&owner).await.unwrap();
        for migration in [
            include_str!("../migrations/001_notes.sql"),
            include_str!("../migrations/002_attachments.sql"),
            include_str!("../migrations/003_note-cache.sql"),
            include_str!("../migrations/005_compliance-access.sql"),
        ] {
            sqlx::raw_sql(migration).execute(&owner).await.unwrap();
        }
        // Fresh login identity: SET ROLE on the owner would retain privileged
        // session_user authority and is intentionally rejected by runtime admission.
        let fixture_role = format!("notes_fixture_{}", Uuid::new_v4().simple());
        let fixture_password = Uuid::new_v4().simple().to_string();
        // Both interpolated tokens contain only internally generated UUID hex.
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE ROLE {fixture_role} LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEROLE IN ROLE fieldnotes_api PASSWORD '{fixture_password}'")))
            .execute(&owner).await.unwrap();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_with(options.username(&fixture_role).password(&fixture_password))
            .await
            .unwrap();
        let mut service = NotesRepository::new(pool, EnvelopeKey::new([73; 32])).unwrap();
        service.cache =
            Some(NoteCache::new(std::env::var("FIELDNOTES_TEST_REDIS_URL").unwrap()).unwrap());
        let scope = verified_scope(Uuid::new_v4()).await;
        let created = service
            .create(&scope, "cache title", "old body")
            .await
            .unwrap();
        let first = service.get(&scope, created.id).await.unwrap();
        assert_eq!(first.body, "old body");
        let old_bytes = service
            .cache
            .as_ref()
            .unwrap()
            .raw(NoteCache::key(scope.tenant_id(), created.id, 1), None)
            .await
            .unwrap();
        service
            .update(&scope, created.id, 1, None, Some("new body"))
            .await
            .unwrap();
        service
            .cache
            .as_ref()
            .unwrap()
            .raw(
                NoteCache::key(scope.tenant_id(), created.id, 1),
                Some(old_bytes),
            )
            .await;
        let current = service.get(&scope, created.id).await.unwrap();
        assert_eq!(current.body, "new body");
        assert_eq!(current.revision, 2);
        let operation: Uuid =
            sqlx::query_scalar("SELECT id FROM fieldnotes_outbox WHERE note_id=$1 AND revision=2")
                .bind(created.id)
                .fetch_one(&owner)
                .await
                .unwrap();
        service
            .apply_insight(
                scope.tenant_id(),
                created.id,
                crate::InsightInput {
                    operation_id: operation,
                    revision: 2,
                    excerpt: "fresh insight".into(),
                    word_count: 2,
                },
            )
            .await
            .unwrap();
        let projected = service.get(&scope, created.id).await.unwrap();
        assert_eq!(projected.insight_excerpt.as_deref(), Some("fresh insight"));
        assert_eq!(projected.revision, 2);
        let wrong = verified_scope(Uuid::new_v4()).await;
        assert!(matches!(
            service.get(&wrong, created.id).await,
            Err(SecurityError::NotFound)
        ));
        service.delete(&scope, created.id).await.unwrap();
        assert!(matches!(
            service.get(&scope, created.id).await,
            Err(SecurityError::NotFound)
        ));
        let cache = service.cache.as_ref().unwrap();
        let url = cache.url.clone();
        let tenant = scope.tenant_id();
        let id = created.id;
        tokio::task::spawn_blocking(move || {
            let mut backend = RedisTtlCache::connect(RedisTtlCacheConfig::new(
                url.as_str(),
                Duration::from_secs(60),
            ))
            .unwrap();
            for version in 1..=3 {
                backend
                    .delete(&NoteCache::key(tenant, id, version))
                    .unwrap();
            }
        })
        .await
        .unwrap();
        service.audit_pool.close().await;
        service.pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP ROLE {fixture_role}")))
            .execute(&owner)
            .await
            .unwrap();
        owner.close().await;
    }
}
