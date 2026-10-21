//! Protected reads acknowledge a tenant-scoped committed audit before plaintext
//! leaves the boundary. Separate bounded audit connections prevent business
//! transactions from exhausting the connections needed to record their reads.
use super::compliance_schema::{
    AttachmentObject, AttachmentStorage, NoteCacheStorage, NoteStorage,
};
use fieldnotes_security::{EnvelopeKey, SecurityError, VerifiedTenant};
use framework_core::{
    common::{FrameworkError, FrameworkResult},
    persistence::protected::{
        AsyncComplianceAudit, ComplianceAccess, ProtectedContext, ProtectedField, ProtectedMember,
    },
};
use sqlx::PgPool;
use uuid::Uuid;

pub(super) struct AccessAudit<'a> {
    pool: &'a PgPool,
    scope: &'a VerifiedTenant,
}
impl<'a> AccessAudit<'a> {
    pub(super) fn new(pool: &'a PgPool, scope: &'a VerifiedTenant) -> Self {
        Self { pool, scope }
    }
    pub(super) fn actor(&self) -> &str {
        self.scope.user_id()
    }
}
fn audit_error() -> FrameworkError {
    FrameworkError::new(
        "compliance_audit_failed",
        "Protected access could not be recorded",
    )
}
impl AsyncComplianceAudit for AccessAudit<'_> {
    fn record<'a>(
        &'a self,
        event: ComplianceAccess<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = FrameworkResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if event.tenant != self.scope.tenant_id() || event.actor != self.scope.user_id() {
                return Err(audit_error());
            }
            let classification = match event.level {
                framework_core::persistence::compliance::ComplianceLevel::Pii => "pii",
                framework_core::persistence::compliance::ComplianceLevel::Phi => "phi",
                framework_core::persistence::compliance::ComplianceLevel::Pci => "pci",
                framework_core::persistence::compliance::ComplianceLevel::None => {
                    return Err(audit_error());
                }
            };
            let operation = async {
                let mut tx = self.pool.begin().await.map_err(|_| audit_error())?;
                sqlx::query("SELECT set_config('fieldnotes.tenant_id',$1,true), set_config('synchronous_commit','on',true), set_config('statement_timeout','2000',true)")
                    .bind(event.tenant.to_string()).execute(&mut *tx).await.map_err(|_|audit_error())?;
                sqlx::query("INSERT INTO fieldnotes_compliance_access(id,workspace_id,resource_id,actor_kind,actor_id,entity,field,classification,reason) VALUES($1,$2,$3,'user',$4,$5,$6,$7,$8)")
                    .bind(Uuid::new_v4()).bind(event.tenant).bind(event.resource).bind(event.actor)
                    .bind(event.entity).bind(event.field).bind(classification).bind(event.reason)
                    .execute(&mut *tx).await.map_err(|_|audit_error())?;
                tx.commit().await.map_err(|_| audit_error())
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), operation)
                .await
                .map_err(|_| audit_error())?
        })
    }
}

fn note_member(purpose: &str) -> Result<ProtectedMember, SecurityError> {
    match purpose {
        "note-title" => NoteStorage::protected_title(),
        "note-body" => NoteStorage::protected_body(),
        "note-insight" => NoteStorage::protected_insight_excerpt(),
        _ => return Err(SecurityError::InvalidInput),
    }
    .map_err(|_| SecurityError::InvalidInput)
}
pub(super) fn seal_note(
    key: &EnvelopeKey,
    tenant: Uuid,
    id: Uuid,
    purpose: &str,
    value: &str,
) -> Result<ProtectedField, SecurityError> {
    ProtectedField::seal(
        key.as_keyring(),
        ProtectedContext::new(tenant, id).map_err(|_| SecurityError::InvalidInput)?,
        note_member(purpose)?,
        value.as_bytes(),
    )
    .map_err(|_| SecurityError::Encryption)
}
#[cfg(test)]
pub(super) async fn reveal_note(
    key: &EnvelopeKey,
    tenant: Uuid,
    id: Uuid,
    purpose: &str,
    value: &str,
    actor: &str,
    audit: &dyn AsyncComplianceAudit,
) -> Result<String, SecurityError> {
    let field = ProtectedField::from_ciphertext(note_member(purpose)?, value.to_owned())
        .map_err(|_| SecurityError::Encryption)?;
    let plain = field
        .reveal_async(
            key.as_keyring(),
            ProtectedContext::new(tenant, id).map_err(|_| SecurityError::InvalidInput)?,
            actor,
            "map-authorized-note",
            audit,
        )
        .await
        .map_err(|_| SecurityError::Unavailable)?;
    String::from_utf8(plain.to_vec()).map_err(|_| SecurityError::Encryption)
}

/// Composite storage slots are classified independently of their decoded DTOs.
/// Metadata contains filename + content type; cache contains the complete note.
/// An attachment's arbitrary user bytes are conservatively protected as PII.
pub(super) enum ProtectedPayload {
    AttachmentMetadata { note: Uuid },
    AttachmentBody { note: Uuid },
    NoteCache { version: i64 },
}
impl ProtectedPayload {
    fn member(self) -> Result<ProtectedMember, SecurityError> {
        match self {
            Self::AttachmentMetadata { note } => ProtectedMember::from_entity::<AttachmentStorage>(
                "metadata",
                format!("attachment-metadata-{note}"),
            ),
            Self::AttachmentBody { note } => ProtectedMember::from_entity::<AttachmentObject>(
                "body",
                format!("attachment-body-{note}"),
            ),
            Self::NoteCache { version } => ProtectedMember::from_entity::<NoteCacheStorage>(
                "snapshot",
                format!("note-cache-v{version}"),
            ),
        }
        .map_err(|_| SecurityError::InvalidInput)
    }
}
pub(super) async fn reveal_payload(
    key: &EnvelopeKey,
    tenant: Uuid,
    id: Uuid,
    payload: ProtectedPayload,
    ciphertext: &str,
    actor: &str,
    audit: &dyn AsyncComplianceAudit,
) -> Result<zeroize::Zeroizing<Vec<u8>>, SecurityError> {
    ProtectedField::from_ciphertext(payload.member()?, ciphertext.to_owned())
        .map_err(|_| SecurityError::Encryption)?
        .reveal_async(
            key.as_keyring(),
            ProtectedContext::new(tenant, id).map_err(|_| SecurityError::InvalidInput)?,
            actor,
            "read-authorized-storage-payload",
            audit,
        )
        .await
        .map_err(|_| SecurityError::Unavailable)
}

#[cfg(test)]
pub(super) struct TestAudit;
#[cfg(test)]
impl AsyncComplianceAudit for TestAudit {
    fn record<'a>(
        &'a self,
        _: ComplianceAccess<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = FrameworkResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn declared_note_members_preserve_storage_purpose_and_reject_cross_field_reads() {
        let key = EnvelopeKey::new([7; 32]);
        let tenant = Uuid::new_v4();
        let id = Uuid::new_v4();
        for purpose in ["note-title", "note-body", "note-insight"] {
            let old = key.seal(tenant, id, purpose, b"private note").unwrap();
            assert_eq!(
                reveal_note(&key, tenant, id, purpose, &old, "test-user", &TestAudit)
                    .await
                    .unwrap(),
                "private note"
            );
            let protected = seal_note(&key, tenant, id, purpose, "private note").unwrap();
            assert_eq!(
                key.open(tenant, id, purpose, protected.ciphertext())
                    .unwrap()
                    .as_slice(),
                b"private note"
            );
            assert!(
                reveal_note(
                    &key,
                    Uuid::new_v4(),
                    id,
                    purpose,
                    protected.ciphertext(),
                    "test-user",
                    &TestAudit
                )
                .await
                .is_err()
            );
        }
        let body = seal_note(&key, tenant, id, "note-body", "private note").unwrap();
        assert!(
            reveal_note(
                &key,
                tenant,
                id,
                "note-title",
                body.ciphertext(),
                "test-user",
                &TestAudit
            )
            .await
            .is_err()
        );
        assert!(seal_note(&key, tenant, id, "unclassified", "private note").is_err());
    }
    #[tokio::test]
    #[ignore = "requires owned disposable fieldnotes_audit_proof PostgreSQL database"]
    async fn durable_audit_is_tenant_bound_append_only_and_survives_business_rollback() {
        use sqlx::Row;
        let db = std::env::var("FIELDNOTES_TEST_AUDIT_DATABASE_URL")
            .expect("owned local audit database");
        let options: sqlx::postgres::PgConnectOptions = db.parse().unwrap();
        assert!(matches!(options.get_host(), "127.0.0.1" | "localhost"));
        assert_eq!(options.get_database(), Some("fieldnotes_audit_proof"));
        let owner = PgPool::connect(&db).await.unwrap();
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
            .max_connections(1)
            .connect_with(options.username(&fixture_role).password(&fixture_password))
            .await
            .unwrap();
        let repo = crate::NotesRepository::new(pool, EnvelopeKey::new([23; 32])).unwrap();
        let scope = super::super::cache::tests::verified_scope(Uuid::new_v4()).await;
        let id = Uuid::new_v4();
        let title = seal_note(
            &repo.key,
            scope.tenant_id(),
            id,
            "note-title",
            "audit-canary-title",
        )
        .unwrap();
        let body = seal_note(
            &repo.key,
            scope.tenant_id(),
            id,
            "note-body",
            "audit-canary-body",
        )
        .unwrap();
        let mut tx = repo.transaction(scope.tenant_id()).await.unwrap();
        let row=sqlx::query("INSERT INTO fieldnotes_notes(id,workspace_id,title,body,created_by) VALUES($1,$2,$3,$4,$5) RETURNING *")
            .bind(id).bind(scope.tenant_id()).bind(title.ciphertext()).bind(body.ciphertext()).bind(scope.user_id()).fetch_one(&mut *tx).await.unwrap();
        // Main pool is exhausted by this transaction; independent audit pool must progress.
        let revealed =
            tokio::time::timeout(std::time::Duration::from_secs(8), repo.record(&scope, &row))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(revealed.body, "audit-canary-body");
        tx.rollback().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM fieldnotes_notes")
            .fetch_one(&owner)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let rows=sqlx::query("SELECT *,row_to_json(fieldnotes_compliance_access)::text AS json FROM fieldnotes_compliance_access ORDER BY field").fetch_all(&owner).await.unwrap();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row.get::<String, _>("actor_id"), scope.user_id());
            assert_eq!(row.get::<String, _>("actor_kind"), "user");
            assert_eq!(row.get::<Uuid, _>("workspace_id"), scope.tenant_id());
            assert!(!row.get::<String, _>("json").contains("audit-canary"));
        }
        let audit = AccessAudit::new(&repo.audit_pool, &scope);
        let wrong = ComplianceAccess {
            entity: "Note",
            field: "body",
            level: framework_core::persistence::compliance::ComplianceLevel::Pii,
            tenant: Uuid::new_v4(),
            resource: id,
            actor: scope.user_id(),
            reason: "test",
        };
        assert!(audit.record(wrong).await.is_err());
        let forged_actor = ComplianceAccess {
            entity: "Note",
            field: "body",
            level: framework_core::persistence::compliance::ComplianceLevel::Pii,
            tenant: scope.tenant_id(),
            resource: id,
            actor: "forged-user",
            reason: "test",
        };
        assert!(audit.record(forged_actor).await.is_err());
        for sql in [
            "SELECT * FROM fieldnotes_compliance_access",
            "DELETE FROM fieldnotes_compliance_access",
            "UPDATE fieldnotes_compliance_access SET reason='changed'",
        ] {
            assert!(sqlx::query(sql).execute(&repo.pool).await.is_err());
        }
        let mut tx = repo.transaction(scope.tenant_id()).await.unwrap();
        assert!(sqlx::query("INSERT INTO fieldnotes_compliance_access(id,workspace_id,resource_id,actor_kind,actor_id,entity,field,classification,reason) VALUES($1,$2,$3,'user','test','Note','body','pii','test')")
            .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(id).execute(&mut *tx).await.is_err());
        tx.rollback().await.unwrap();
        repo.audit_pool.close().await;
        assert!(
            reveal_note(
                &repo.key,
                scope.tenant_id(),
                id,
                "note-body",
                body.ciphertext(),
                scope.user_id(),
                &audit
            )
            .await
            .is_err()
        );
        repo.pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP ROLE {fixture_role}")))
            .execute(&owner)
            .await
            .unwrap();
        owner.close().await;
    }
}
