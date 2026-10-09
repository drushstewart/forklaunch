//! Test-only provisioning for the generated IAM runtime fixture.
use forklaunch_core::persistence::TenantContext;
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
        for table in ["fl_iam_sessions", "fl_iam_users", "fl_iam_organizations", "fl_iam_roles"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE tenant_id=$1 OR tenant_id=$2"))
                .bind(format!("{role}-a")).bind(format!("{role}-b")).execute(&owner).await?;
        }
        sqlx::query(&format!("DROP OWNED BY {role}")).execute(&owner).await?;
        sqlx::query(&format!("DROP ROLE {role}")).execute(&owner).await?;
    } else {
        sqlx::query(&format!("CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD 'parity-test-only'"))
            .execute(&owner).await?;
        sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {role}")).execute(&owner).await?;
        sqlx::query(&format!("GRANT SELECT,INSERT,UPDATE,DELETE ON fl_iam_users,fl_iam_organizations,fl_iam_roles,fl_iam_sessions TO {role}"))
            .execute(&owner).await?;
        let set_role = format!("SET ROLE {role}");
        let app = PgPoolOptions::new().max_connections(2).after_connect(move |connection, _| {
            let set_role = set_role.clone();
            Box::pin(async move { sqlx::query(&set_role).execute(connection).await?; Ok(()) })
        }).connect(&owner_url).await?;
        let first = PostgresIamService::new(app.clone(), TenantContext::new(format!("{role}-a"))?).await?;
        let second = first.for_tenant(TenantContext::new(format!("{role}-b"))?);
        seed(&first, "First", "first-password").await?;
        seed(&second, "Second", "second-password").await?;
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
