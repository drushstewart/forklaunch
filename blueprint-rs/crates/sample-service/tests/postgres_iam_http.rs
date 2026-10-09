#![cfg(feature = "postgres")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use forklaunch_blueprint_sample_service::async_iam::session_iam_router;
use forklaunch_core::persistence::TenantContext;
use forklaunch_iam_base::postgres::PostgresIamService;
use forklaunch_iam_interface::{AsyncIamRepository, Organization, Permission, Role, User};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use tower::ServiceExt;

struct Fixture {
    owner: PgPool,
    pool: PgPool,
    role: String,
    first: Arc<PostgresIamService>,
    second: Arc<PostgresIamService>,
}

impl Fixture {
    async fn new() -> Self {
        let url =
            std::env::var("FORKLAUNCH_TEST_POSTGRES_URL").expect("PostgreSQL test URL required");
        let owner = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        PostgresIamService::migrate(&owner).await.unwrap();
        assert_eq!(
            PostgresIamService::new(owner.clone(), TenantContext::new("unsafe-role").unwrap())
                .await
                .err()
                .unwrap()
                .code,
            "iam_unsafe_database_role"
        );
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let role = format!("fl_http_{}_{nonce}", std::process::id());
        sqlx::query(&format!(
            "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOBYPASSRLS"
        ))
        .execute(&owner)
        .await
        .unwrap();
        sqlx::query(&format!("GRANT USAGE ON SCHEMA public TO {role}"))
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query(&format!("GRANT SELECT,INSERT,UPDATE,DELETE ON fl_iam_users,fl_iam_organizations,fl_iam_roles,fl_iam_sessions TO {role}")).execute(&owner).await.unwrap();
        let set_role = format!("SET ROLE {role}");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .after_connect(move |connection, _| {
                let set_role = set_role.clone();
                Box::pin(async move {
                    sqlx::query(&set_role).execute(connection).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        let first = Arc::new(
            PostgresIamService::new(
                pool.clone(),
                TenantContext::new(format!("{role}-a")).unwrap(),
            )
            .await
            .unwrap(),
        );
        let second = Arc::new(first.for_tenant(TenantContext::new(format!("{role}-b")).unwrap()));
        seed(first.as_ref(), "First", "first-password").await;
        seed(second.as_ref(), "Second", "second-password").await;
        Self {
            owner,
            pool,
            role,
            first,
            second,
        }
    }

    async fn close(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP OWNED BY {}", self.role))
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query(&format!("DROP ROLE {}", self.role))
            .execute(&self.owner)
            .await
            .unwrap();
        self.owner.close().await;
    }
}

// Seed through public async traits, exercising the production implementation.
async fn seed<S: AsyncIamRepository>(service: &S, label: &str, password: &str) {
    service
        .create_user(
            User::new("user-1", "member@example.com")
                .with_display_name(label)
                .with_role("member"),
            password.to_owned(),
        )
        .await
        .unwrap();
    service
        .create_user(
            User::new("user-2", "other@example.com"),
            password.to_owned(),
        )
        .await
        .unwrap();
    service
        .put_role(Role::new("member").with_permission(Permission::new("users", "read")))
        .await
        .unwrap();
    service
        .create_organization(Organization::new("org-1", label, "org"))
        .await
        .unwrap();
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("x-correlation-id", "iam-http-test");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = body.map_or_else(Body::empty, |body| {
        request = std::mem::take(&mut request).header("content-type", "application/json");
        Body::from(body.to_string())
    });
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    if method == "POST" && path == "/auth/login" && status == 200 {
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL; exercised in parity CI"]
async fn postgres_iam_http_login_pagination_tenant_isolation_and_revocation() {
    let fixture = Fixture::new().await;
    let first = session_iam_router(Arc::clone(&fixture.first)).into_router();
    let second = session_iam_router(Arc::clone(&fixture.second)).into_router();
    assert_eq!(call(&first, "GET", "/users", None, None).await.0, 401);
    assert_eq!(
        call(
            &first,
            "POST",
            "/auth/login",
            None,
            Some(json!({"email":"member@example.com", "password":"wrong"}))
        )
        .await
        .0,
        401
    );
    assert_eq!(
        call(
            &first,
            "POST",
            "/auth/login",
            None,
            Some(json!({"email":123}))
        )
        .await
        .0,
        400
    );
    let (status, session) = call(
        &first,
        "POST",
        "/auth/login",
        None,
        Some(json!({"email":"member@example.com", "password":"first-password"})),
    )
    .await;
    assert_eq!(status, 200);
    let token = session["id"].as_str().unwrap();
    assert_eq!(
        call(&second, "GET", "/users", Some(token), None).await.0,
        401
    );
    assert_eq!(call(&first, "HEAD", "/users", None, None).await.0, 401);
    assert_authenticated_reads(&first, token).await;
    assert_eq!(
        call(&first, "POST", "/auth/logout", Some(token), None)
            .await
            .0,
        204
    );
    assert_eq!(
        call(&first, "GET", "/users", Some(token), None).await.0,
        401
    );
    fixture.pool.close().await;
    let (status, error) = call(
        &first,
        "POST",
        "/auth/login",
        None,
        Some(json!({"email":"member@example.com", "password":"first-password"})),
    )
    .await;
    assert_eq!(status, 500);
    assert!(!error.to_string().contains("pool"));
    assert!(!error.to_string().contains("postgres"));
    fixture.close().await;
}

async fn assert_authenticated_reads(first: &Router, token: &str) {
    let (status, page) = call(first, "GET", "/users?limit=1", Some(token), None).await;
    assert_eq!(status, 200);
    assert_eq!(page["items"][0]["display_name"], "First");
    assert_eq!(page["next_cursor"], "user-1");
    let (_, page) = call(
        first,
        "GET",
        "/users?limit=1&cursor=user-1",
        Some(token),
        None,
    )
    .await;
    assert_eq!(page["items"][0]["id"], "user-2");
    assert!(page["next_cursor"].is_null());
    for query in ["limit=0", "limit=1001", "limit=invalid"] {
        assert_eq!(
            call(first, "GET", &format!("/users?{query}"), Some(token), None)
                .await
                .0,
            400
        );
    }
    assert_eq!(
        call(first, "GET", "/users/missing", Some(token), None)
            .await
            .0,
        404
    );
    let (_, permissions) = call(first, "GET", "/users/user-1/permissions", Some(token), None).await;
    assert_eq!(permissions, json!([{"resource":"users", "action":"read"}]));
    let (_, organizations) = call(first, "GET", "/organizations?limit=1", Some(token), None).await;
    assert_eq!(organizations["items"][0]["name"], "First");
    let (_, openapi) = call(first, "GET", "/openapi.json", None, None).await;
    assert_eq!(
        openapi["components"]["securitySchemes"]["bearerAuth"]["scheme"],
        "bearer"
    );
}
