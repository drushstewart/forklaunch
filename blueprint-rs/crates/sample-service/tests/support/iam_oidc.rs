use super::*;
use forklaunch_auth::oidc::OidcProvider;
use forklaunch_core::persistence::encryption::FieldEncryptor;
use forklaunch_iam_axum::browser::BrowserIam;
use forklaunch_iam_base::browser_sessions::PostgresCookieStore;
#[path = "iam_oidc/provider.rs"]
mod provider;
use provider::ProviderFixture;
const ORIGIN: &str = "https://iam.example.test";
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    proof: &str,
    body: Value,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(format!("/auth/browser/{path}"));
    if !cookie.is_empty() {
        req = req.header("cookie", cookie);
    }
    if method == "POST" {
        req = req
            .header("origin", ORIGIN)
            .header("x-csrf-token", proof)
            .header("content-type", "application/json");
    }
    app.clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
async fn body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap()
}
async fn bootstrap(app: &Router) -> (String, String) {
    let response = request(app, "GET", "csrf", "", "", json!(null)).await;
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let proof = body(response).await["csrfToken"]
        .as_str()
        .unwrap()
        .to_owned();
    (cookie, proof)
}
async fn begin(app: &Router, cookie: &str, proof: &str) -> Value {
    let response = request(app, "POST", "oidc/start", cookie, proof, json!(null)).await;
    assert_eq!(response.status(), 200);
    body(response).await
}
fn provider_response(fixture: &ProviderFixture, response: &Value) -> String {
    let url = response["authorizationUrl"].as_str().unwrap();
    let params: std::collections::HashMap<_, _> =
        form_urlencoded::parse(url.split_once('?').unwrap().1.as_bytes())
            .into_owned()
            .collect();
    assert_eq!(params["code_challenge_method"], "S256");
    fixture.token(&params["nonce"], "subject");
    params["state"].clone()
}
#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
#[allow(clippy::too_many_lines)] // One browser lifecycle shares rotated cookies and single-use state.
async fn postgres_iam_oidc_browser_start_callback_rotation_and_replay() {
    let fixture = Fixture::new().await;
    let remote = ProviderFixture::new().await;
    let provider = Arc::new(
        OidcProvider::discover(
            &remote.issuer,
            "local-client".into(),
            None,
            &format!("{ORIGIN}/auth/browser/oidc/callback"),
        )
        .await
        .unwrap(),
    );
    let store = PostgresCookieStore::new(
        fixture.pool.clone(),
        TenantContext::new(format!("{}-a", fixture.role)).unwrap(),
        "oidc-http",
        Arc::new(FieldEncryptor::new("oidc-http-fixture").unwrap()),
    )
    .await
    .unwrap();
    let make_app = || {
        BrowserIam::new(fixture.first.clone(), store.clone(), "__Host-iam", ORIGIN)
            .unwrap()
            .with_oidc(
                fixture.first.clone(),
                store.clone(),
                provider.clone(),
                vec![],
            )
            .unwrap()
            .into_router()
    };
    let app = make_app();
    let (cookie, proof) = bootstrap(&app).await;
    assert_eq!(
        request(&app, "POST", "oidc/start", &cookie, "wrong", json!(null))
            .await
            .status(),
        403
    );
    let state = provider_response(&remote, &begin(&app, &cookie, &proof).await);
    // Same email from a provider never provisions or links a local account.
    assert_eq!(
        request(
            &app,
            "POST",
            "oidc/callback",
            &cookie,
            &proof,
            json!({"code":"code","state":state})
        )
        .await
        .status(),
        401
    );
    fixture
        .first
        .bind_provider_identity(&remote.issuer, "subject", "user-1")
        .await
        .unwrap();
    let state = provider_response(&remote, &begin(&app, &cookie, &proof).await);
    let (other, other_proof) = bootstrap(&app).await;
    assert_eq!(
        request(
            &app,
            "POST",
            "oidc/callback",
            &other,
            &other_proof,
            json!({"code":"code","state":state})
        )
        .await
        .status(),
        401
    );
    let restarted = make_app();
    let response = request(
        &restarted,
        "POST",
        "oidc/callback",
        &cookie,
        &proof,
        json!({"code":"code","state":state}),
    )
    .await;
    assert_eq!(response.status(), 200);
    let rotated = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        rotated.contains("Secure")
            && rotated.contains("HttpOnly")
            && rotated.contains("SameSite=Strict")
    );
    let rotated = rotated.split(';').next().unwrap().to_owned();
    assert_ne!(cookie, rotated);
    let next_proof = body(response).await["csrfToken"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(proof, next_proof);
    assert_eq!(
        request(&restarted, "GET", "session", &rotated, "", json!(null))
            .await
            .status(),
        200
    );
    assert_eq!(
        request(
            &restarted,
            "POST",
            "oidc/callback",
            &rotated,
            &next_proof,
            json!({"code":"code","state":state})
        )
        .await
        .status(),
        401
    );
    check_relay(&app).await;
    assert_eq!(
        request(
            &restarted,
            "POST",
            "logout",
            &rotated,
            &next_proof,
            json!(null)
        )
        .await
        .status(),
        204
    );
    fixture.close().await;
}
async fn check_relay(app: &Router) {
    let response = request(
        app,
        "GET",
        "oidc/callback?code=secret-code&state=secret-state",
        "",
        "",
        json!(null),
    )
    .await;
    assert_eq!(response.status(), 200);
    assert!(!response.headers().contains_key("set-cookie"));
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("script-src 'sha256-")
    );
    let html = String::from_utf8(
        to_bytes(response.into_body(), 16384)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(!html.contains("secret-code"));
    assert!(!html.contains("secret-state"));
    assert!(html.find("history.replaceState").unwrap() < html.find("fetch(").unwrap());
}
