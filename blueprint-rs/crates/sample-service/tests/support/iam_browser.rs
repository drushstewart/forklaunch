use super::*;
use axum::response::Response;
use forklaunch_core::persistence::encryption::FieldEncryptor;
use forklaunch_iam_axum::browser::BrowserIam;
use forklaunch_iam_base::browser_sessions::PostgresCookieStore;

const ORIGIN: &str = "https://iam.example.test";

async fn browser_call(
    app: &Router,
    method: &str,
    endpoint: &str,
    cookie: &str,
    proof: &str,
    origin: &str,
    password: &str,
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/auth/browser/{endpoint}"));
    for (name, value) in [
        ("cookie", cookie),
        ("x-csrf-token", proof),
        ("origin", origin),
    ] {
        if !value.is_empty() {
            request = request.header(name, value);
        }
    }
    let body = if endpoint == "login" && method == "POST" {
        request = request.header("content-type", "application/json");
        Body::from(json!({"email":"member@example.com","password":password}).to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    response
}

fn browser_cookie(response: &Response) -> String {
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    for attribute in ["Secure", "HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(cookie.contains(attribute), "{attribute}");
    }
    cookie.split(';').next().unwrap().to_owned()
}

async fn browser_json(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()
}

async fn active_sessions(fixture: &Fixture) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM fl_iam_sessions WHERE tenant_id=$1 AND expires_at > now()",
    )
    .bind(format!("{}-a", fixture.role))
    .fetch_one(&fixture.owner)
    .await
    .unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One browser lifecycle shares its rotated cookie and CSRF proof.
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL; exercised in parity CI"]
async fn postgres_iam_http_browser_cookie_csrf_rotation_and_revocation() {
    let fixture = Fixture::new().await;
    let store = PostgresCookieStore::new(
        fixture.pool.clone(),
        TenantContext::new(format!("{}-a", fixture.role)).unwrap(),
        "browser-lifecycle",
        Arc::new(FieldEncryptor::new("browser-fixture-only").unwrap()),
    )
    .await
    .unwrap();
    let browser =
        BrowserIam::new(fixture.first.clone(), store.clone(), "__Host-iam", ORIGIN).unwrap();
    let app = browser.into_router();
    assert!(BrowserIam::new(fixture.first.clone(), store.clone(), "bad cookie", ORIGIN).is_err());
    assert!(
        BrowserIam::new(
            fixture.first.clone(),
            store.clone(),
            "__Host-iam",
            "http://iam.example.test"
        )
        .is_err()
    );
    assert_eq!(
        browser_call(&app, "GET", "session", "", "", "", "")
            .await
            .status(),
        401
    );
    let csrf = browser_call(&app, "GET", "csrf", "", "", "", "").await;
    let anonymous = browser_cookie(&csrf);
    let proof = browser_json(csrf).await["csrfToken"]
        .as_str()
        .unwrap()
        .to_owned();
    for (origin, token) in [
        ("", proof.as_str()),
        ("https://evil.test", proof.as_str()),
        (ORIGIN, "wrong"),
    ] {
        assert_eq!(
            browser_call(
                &app,
                "POST",
                "login",
                &anonymous,
                token,
                origin,
                "first-password"
            )
            .await
            .status(),
            403
        );
        assert_eq!(active_sessions(&fixture).await, 0);
    }
    assert_eq!(
        browser_call(&app, "GET", "login", &anonymous, &proof, ORIGIN, "")
            .await
            .status(),
        405
    );
    assert_eq!(
        browser_call(&app, "POST", "login", &anonymous, &proof, ORIGIN, "wrong")
            .await
            .status(),
        401
    );
    assert_eq!(active_sessions(&fixture).await, 0);
    let login = browser_call(
        &app,
        "POST",
        "login",
        &anonymous,
        &proof,
        ORIGIN,
        "first-password",
    )
    .await;
    assert_eq!(login.status(), 200);
    let cookie = browser_cookie(&login);
    assert_ne!(cookie, anonymous);
    let payload = browser_json(login).await;
    assert_eq!(payload.as_object().unwrap().len(), 1);
    let proof2 = payload["csrfToken"].as_str().unwrap();
    assert_ne!(proof2, proof);
    assert_eq!(active_sessions(&fixture).await, 1);
    assert_eq!(
        browser_call(&app, "GET", "session", &anonymous, "", "", "")
            .await
            .status(),
        401
    );
    let current = browser_call(&app, "GET", "session", &cookie, "", "", "").await;
    assert_eq!(current.status(), 200);
    let claims = browser_json(current).await;
    assert_eq!(claims["sub"], "user-1");
    assert_eq!(claims["permissions"], json!(["users:read"]));
    assert!(claims.get("id").is_none());
    assert!(claims.get("password").is_none());
    // Reusing the store across application instances preserves the login, while
    // the other tenant's backing ledger must still reject the same cookie.
    let restarted = BrowserIam::new(fixture.first.clone(), store.clone(), "__Host-iam", ORIGIN)
        .unwrap()
        .into_router();
    let other = BrowserIam::new(fixture.second.clone(), store.clone(), "__Host-iam", ORIGIN)
        .unwrap()
        .into_router();
    assert_eq!(
        browser_call(&restarted, "GET", "session", &cookie, "", "", "")
            .await
            .status(),
        200
    );
    assert_eq!(
        browser_call(&other, "GET", "session", &cookie, "", "", "")
            .await
            .status(),
        401
    );
    fixture
        .first
        .put_role(Role::new("member").with_permission(Permission::new("users", "write")))
        .await
        .unwrap();
    assert_eq!(
        browser_json(browser_call(&app, "GET", "session", &cookie, "", "", "").await).await["permissions"],
        json!(["users:write"])
    );
    let mut user = fixture.first.get_user("user-1").await.unwrap().unwrap();
    user.disabled = true;
    fixture.first.update_user(user.clone()).await.unwrap();
    assert_eq!(
        browser_call(&app, "GET", "session", &cookie, "", "", "")
            .await
            .status(),
        401
    );
    user.disabled = false;
    fixture.first.update_user(user).await.unwrap();
    // A second login rotates both proofs and revokes its previous ledger token.
    let login = browser_call(
        &app,
        "POST",
        "login",
        &cookie,
        proof2,
        ORIGIN,
        "first-password",
    )
    .await;
    assert_eq!(login.status(), 200);
    let cookie2 = browser_cookie(&login);
    let payload = browser_json(login).await;
    let proof3 = payload["csrfToken"].as_str().unwrap();
    assert_ne!(cookie2, cookie);
    assert_eq!(active_sessions(&fixture).await, 1);
    assert_eq!(
        browser_call(&app, "POST", "logout", &cookie2, proof2, ORIGIN, "")
            .await
            .status(),
        403
    );
    assert_eq!(
        browser_call(&app, "POST", "logout", &cookie2, proof3, ORIGIN, "")
            .await
            .status(),
        204
    );
    assert_eq!(active_sessions(&fixture).await, 0);
    assert_eq!(
        browser_call(&app, "GET", "session", &cookie2, "", "", "")
            .await
            .status(),
        401
    );
    // Password changes invalidate cookies through the persistent IAM ledger.
    let csrf = browser_call(&app, "GET", "csrf", "", "", "", "").await;
    let anonymous = browser_cookie(&csrf);
    let proof = browser_json(csrf).await["csrfToken"]
        .as_str()
        .unwrap()
        .to_owned();
    let login = browser_call(
        &app,
        "POST",
        "login",
        &anonymous,
        &proof,
        ORIGIN,
        "first-password",
    )
    .await;
    let cookie = browser_cookie(&login);
    fixture
        .first
        .set_password("user-1", "changed-password".to_owned())
        .await
        .unwrap();
    assert_eq!(
        browser_call(&app, "GET", "session", &cookie, "", "", "")
            .await
            .status(),
        401
    );
    drop((app, restarted, other));
    fixture.close().await;
}
