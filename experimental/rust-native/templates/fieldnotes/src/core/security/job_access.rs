//! Service-only audit adapter for classified encrypted job input/results.
use sqlx::PgPool;
use std::{future::Future, pin::Pin};
use worker::{JobAccess, JobAccessAudit, ProcessorError};

#[derive(Clone, Copy)]
pub enum ServiceActor {
    Notes,
    Insights,
}
impl ServiceActor {
    pub fn name(self) -> &'static str {
        match self {
            Self::Notes => "service:notes",
            Self::Insights => "service:insights",
        }
    }
}
pub struct ServiceJobAudit {
    pool: PgPool,
    actor: ServiceActor,
    purpose: &'static str,
    entity: &'static str,
}
impl ServiceJobAudit {
    pub fn new(
        pool: PgPool,
        actor: ServiceActor,
        purpose: &'static str,
        entity: &'static str,
    ) -> Self {
        Self {
            pool,
            actor,
            purpose,
            entity,
        }
    }
}
impl JobAccessAudit for ServiceJobAudit {
    fn record<'a>(
        &'a self,
        access: JobAccess<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<(), ProcessorError>> + Send + 'a>> {
        Box::pin(async move {
            if access.actor != self.actor.name()
                || access.purpose != self.purpose
                || access.context.tenant_id.is_nil()
            {
                return Err(ProcessorError::Failed);
            }
            let write = async {
                let mut tx = self
                    .pool
                    .begin()
                    .await
                    .map_err(|_| ProcessorError::Failed)?;
                sqlx::query("SELECT set_config('fieldnotes.tenant_id',$1,true),set_config('synchronous_commit','on',true),set_config('statement_timeout','2000',true)")
                    .bind(access.context.tenant_id.to_string()).execute(&mut *tx).await.map_err(|_|ProcessorError::Failed)?;
                sqlx::query("INSERT INTO fieldnotes_compliance_access(id,workspace_id,resource_id,actor_kind,actor_id,entity,field,classification,reason,operation_id,purpose) VALUES($1,$2,$3,'service',$4,$5,'payload','pii','read-classified-job',$6,$7)")
                    .bind(uuid::Uuid::new_v4()).bind(access.context.tenant_id).bind(access.context.resource_id)
                    .bind(access.actor).bind(self.entity).bind(access.context.operation_id).bind(access.purpose)
                    .execute(&mut *tx).await.map_err(|_|ProcessorError::Failed)?;
                tx.commit().await.map_err(|_| ProcessorError::Failed)
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), write)
                .await
                .map_err(|_| ProcessorError::Failed)?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;
    use worker::{JobContext, JobPayload};
    use zeroize::Zeroize;
    #[derive(Serialize, Deserialize)]
    struct Payload {
        context: JobContext,
        secret: String,
    }
    impl Zeroize for Payload {
        fn zeroize(&mut self) {
            self.secret.zeroize();
        }
    }
    impl JobPayload for Payload {
        fn context(&self) -> JobContext {
            self.context
        }
    }
    #[tokio::test]
    #[ignore = "requires owned local fieldnotes_service_audit_proof PostgreSQL database"]
    async fn service_plaintext_requires_committed_role_scoped_audit() {
        let url = std::env::var("FIELDNOTES_SERVICE_AUDIT_DATABASE_URL").unwrap();
        let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
        assert!(matches!(options.get_host(), "localhost" | "127.0.0.1"));
        assert_eq!(
            options.get_database(),
            Some("fieldnotes_service_audit_proof")
        );
        let owner = PgPool::connect(&url).await.unwrap();
        sqlx::raw_sql("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='fieldnotes_api') THEN CREATE ROLE fieldnotes_api NOLOGIN NOSUPERUSER NOBYPASSRLS; END IF; IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='fieldnotes_worker') THEN CREATE ROLE fieldnotes_worker NOLOGIN NOSUPERUSER NOBYPASSRLS; END IF; END $$;").execute(&owner).await.unwrap();
        for migration in [
            include_str!("../../notes/migrations/005_compliance-access.sql"),
            include_str!("../../notes/migrations/006_service-job-access.sql"),
        ] {
            sqlx::raw_sql(migration).execute(&owner).await.unwrap();
        }
        let api = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options.clone().options([("role", "fieldnotes_api")]))
            .await
            .unwrap();
        let worker = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options.options([("role", "fieldnotes_worker")]))
            .await
            .unwrap();
        let key = crate::EnvelopeKey::new([22; 32]);
        let context = JobContext {
            tenant_id: Uuid::new_v4(),
            resource_id: Uuid::new_v4(),
            operation_id: Uuid::new_v4(),
            revision: 1,
        };
        let payload = Payload {
            context,
            secret: "service-audit-private-canary".into(),
        };
        let ciphertext = worker::seal_job(key.as_keyring(), "job-proof", &payload).unwrap();
        for (pool, actor) in [
            (api.clone(), ServiceActor::Notes),
            (worker.clone(), ServiceActor::Insights),
        ] {
            let audit = ServiceJobAudit::new(pool.clone(), actor, "job-proof", "JobProof");
            let transaction = pool.begin().await.unwrap();
            assert_eq!(
                worker::decode_job_audited::<Payload>(
                    key.as_keyring(),
                    context.tenant_id,
                    context,
                    "job-proof",
                    &ciphertext,
                    actor.name(),
                    &audit
                )
                .await
                .unwrap()
                .secret,
                payload.secret
            );
            transaction.rollback().await.unwrap();
            assert!(
                worker::decode_job_audited::<Payload>(
                    key.as_keyring(),
                    Uuid::new_v4(),
                    context,
                    "job-proof",
                    &ciphertext,
                    actor.name(),
                    &audit
                )
                .await
                .is_err()
            );
            assert!(
                worker::decode_job_audited::<Payload>(
                    key.as_keyring(),
                    context.tenant_id,
                    context,
                    "job-proof",
                    &ciphertext,
                    "forged-service",
                    &audit
                )
                .await
                .is_err()
            );
            for sql in [
                "SELECT * FROM fieldnotes_compliance_access",
                "DELETE FROM fieldnotes_compliance_access",
                "UPDATE fieldnotes_compliance_access SET actor_id='changed'",
            ] {
                assert!(sqlx::query(sql).execute(&pool).await.is_err());
            }
        }
        let wrong_role =
            ServiceJobAudit::new(worker.clone(), ServiceActor::Notes, "job-proof", "JobProof");
        assert!(
            worker::decode_job_audited::<Payload>(
                key.as_keyring(),
                context.tenant_id,
                context,
                "job-proof",
                &ciphertext,
                "service:notes",
                &wrong_role
            )
            .await
            .is_err()
        );
        let records:Vec<(String,String)>=sqlx::query_as("SELECT actor_id,to_jsonb(a)::text FROM fieldnotes_compliance_access a ORDER BY actor_id").fetch_all(&owner).await.unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, "service:insights");
        assert_eq!(records[1].0, "service:notes");
        assert!(
            records
                .iter()
                .all(|(_, json)| !json.contains("private-canary") && !json.contains(&ciphertext))
        );
        worker.close().await;
        let unavailable =
            ServiceJobAudit::new(worker, ServiceActor::Insights, "job-proof", "JobProof");
        assert!(
            worker::decode_job_audited::<Payload>(
                key.as_keyring(),
                context.tenant_id,
                context,
                "job-proof",
                &ciphertext,
                "service:insights",
                &unavailable
            )
            .await
            .is_err()
        );
        api.close().await;
        owner.close().await;
    }
}
