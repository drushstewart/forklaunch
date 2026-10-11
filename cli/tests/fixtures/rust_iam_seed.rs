//! Test-only provisioning for the generated IAM runtime fixture.
use forklaunch_core::persistence::{TenantContext,encryption::FieldEncryptor};
use forklaunch_iam_base::directory::{postgres::PostgresDirectoryStore,service::BaseIamDirectory};
use forklaunch_blueprint_core::AuditContext;
use forklaunch_iam_interface::services::{UserService,RoleService,PermissionService,OrganizationService};
use std::sync::Arc;
use serde_json::{json,Value};
use forklaunch_iam_base::postgres::PostgresIamService;
use forklaunch_iam_interface::{AsyncIamRepository, Organization, Permission, Role, User};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let owner_url = std::env::var("FORKLAUNCH_TEST_POSTGRES_URL")?;
    let role = std::env::var("FORKLAUNCH_TEST_ROLE")?;
    assert!(role.starts_with("fl_gen_") && role.len() < 63 && role.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    let owner = PgPoolOptions::new().max_connections(2).connect(&owner_url).await?;
    if std::env::args().any(|arg| arg == "--cleanup") {
        for table in ["fl_iam_sessions", "fl_iam_users", "fl_iam_organizations", "fl_iam_roles", "fl_iam_directory", "fl_iam_directory_audit"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE tenant_id=$1 OR tenant_id=$2"))
                .bind(format!("{role}-a")).bind(format!("{role}-b")).execute(&owner).await?;
        }
        sqlx::query(&format!("DROP OWNED BY {role}")).execute(&owner).await?;
        sqlx::query(&format!("DROP ROLE {role}")).execute(&owner).await?;
    } else {
        sqlx::query(&format!("CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'parity-test-only'"))
            .execute(&owner).await?;
        sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {role}")).execute(&owner).await?;
        sqlx::query(&format!("GRANT SELECT,INSERT,UPDATE,DELETE ON fl_iam_users,fl_iam_organizations,fl_iam_roles,fl_iam_sessions,fl_iam_directory,fl_iam_directory_unique_claims,fl_iam_directory_audit TO {role}"))
            .execute(&owner).await?;
        sqlx::query(&format!("GRANT USAGE ON SEQUENCE fl_iam_directory_sequence_seq TO {role}")).execute(&owner).await?;
        let set_role = format!("SET ROLE {role}");
        let app = PgPoolOptions::new().max_connections(2).after_connect(move |connection, _| {
            let set_role = set_role.clone();
            Box::pin(async move { sqlx::query(&set_role).execute(connection).await?; Ok(()) })
        }).connect(&owner_url).await?;
        let first = PostgresIamService::new(app.clone(), TenantContext::new(format!("{role}-a"))?).await?;
        let second = first.for_tenant(TenantContext::new(format!("{role}-b"))?);
        seed(&first, "First", "first-password").await?;
        seed(&second, "Second", "second-password").await?;
        let mut directory=serde_json::Map::new();
        for suffix in ["a","b"] {
            let store=PostgresDirectoryStore::new(app.clone(),TenantContext::new(format!("{role}-{suffix}"))?,Arc::new(FieldEncryptor::new("iam-fixture-encryption-only")?),AuditContext::system("fixture-seed")).await?;
            directory.insert(suffix.into(),seed_directory(BaseIamDirectory::new(Arc::new(store))).await?);
        }
        std::fs::write("iam-directory-fixture.json",serde_json::to_vec(&directory)?)?;
        app.close().await;
    }
    owner.close().await;
    Ok(())
}

async fn seed<S: AsyncIamRepository>(service: &S, label: &str, password: &str) -> Result<(), Box<dyn std::error::Error>> {
    service.create_user(User::new("user-1", "member@example.com").with_display_name(label).with_role("member"), password.to_owned()).await?;
    service.create_user(User::new("user-2", "other@example.com"), password.to_owned()).await?;
    service.put_role(Role::new("member").with_permission(Permission::new("users", "read"))).await?;
    service.create_organization(Organization::new("org-1", label, "org")).await?;
    Ok(())
}

async fn seed_directory(service:BaseIamDirectory<PostgresDirectoryStore>) -> Result<Value,Box<dyn std::error::Error>> {
    let permission=service.create_permission(serde_json::from_value(json!({"slug":"platform:read"}))?).await?;
    let role=service.create_role(serde_json::from_value(json!({"name":"Readers","permissionIds":[permission.id]}))?).await?;
    let organization=service.create_organization(serde_json::from_value(json!({"name":"Directory organization","domain":"fixture.test","subscription":"fixture-subscription"}))?).await?;
    let outsider_org=service.create_organization(serde_json::from_value(json!({"name":"Other organization","domain":"other.fixture.test","subscription":"other-subscription"}))?).await?;
    let mut users=Vec::new();
    for (index,organization) in [organization.id.clone(),organization.id.clone(),outsider_org.id.clone()].into_iter().enumerate() {
        users.push(service.create_user(serde_json::from_value(json!({"email":format!("directory{index}@fixture.test"),"password":"unused","firstName":"Directory","lastName":"Member","organization":organization,"roles":[role.id]}))?).await?);
    }
    Ok(json!({"permission":permission,"role":role,"organization":organization,"outsiderOrganization":outsider_org,"users":users}))
}
