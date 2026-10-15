use super::*;
use crate::store::{AuthTransaction, Model, memory::MemoryAuthStore};
use axum::body::to_bytes;
use chrono::Duration;
use tower::ServiceExt;

#[derive(Default)]
struct Delivery(std::sync::Mutex<String>);
impl crate::two_factor::OtpDelivery for Delivery {
    fn prepare(
        &self,
        context: crate::two_factor::OtpDeliveryContext,
    ) -> BlueprintResult<crate::two_factor::OtpDeliveryFuture> {
        *self.0.lock().unwrap() = context.code().to_owned();
        Ok(Box::pin(async { Ok(()) }))
    }
}
async fn call(
    router: &Router,
    path: &str,
    body: Option<Value>,
    cookie: &str,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(path)
        .header("origin", "http://localhost:3000")
        .header("cookie", cookie);
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            request
                .body(Body::from(body.map_or_else(String::new, |v| v.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
    (
        parts.status,
        parts.headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn cookies(headers: &HeaderMap) -> String {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|h| h.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ")
}
fn expires(row: &Value) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(row["expiresAt"].as_str().unwrap())
        .unwrap()
        .with_timezone(&Utc)
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let policy = crate::sessions::SessionPolicy::new(120, 30, 60, false).unwrap();
    let clock = Arc::new(std::sync::atomic::AtomicI64::new(now.timestamp_millis()));
    let delivery = Arc::new(Delivery::default());
    let app = AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![7; 32],
        vec![],
    )
    .unwrap()
    .with_session_policy(policy)
    .with_two_factor()
    .unwrap()
    .with_two_factor_lifecycle_options(
        crate::two_factor::TwoFactorLifecycleOptions::default()
            .with_skip_verification_on_enable(true),
    )
    .unwrap()
    .with_two_factor_otp(delivery.clone())
    .unwrap()
    // Exercise both builder orders, including cloning an installed factor config.
    .with_session_policy(policy)
    .with_auto_sign_in_after_verification(true)
    .with_rate_limiting(false)
    .with_clock({
        let clock = clock.clone();
        move || {
            DateTime::from_timestamp_millis(clock.load(std::sync::atomic::Ordering::SeqCst))
                .unwrap()
        }
    })
    .router();
    let email = "http-policy@example.test";
    let signup = json!({"email":email,"password":"owned-password","name":"Owned","firstName":"Owned","lastName":"Policy"});
    let (status, headers, body) = call(&app, "/sign-up/email", Some(signup), "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=120")
    );
    let mut cookie = cookies(&headers);
    let mut tx = store.begin(false).await.unwrap();
    let session = tx
        .find_unique(Model::Session, "token", body["token"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(expires(&session), now + Duration::seconds(120));
    clock.store(
        (now + Duration::seconds(30)).timestamp_millis(),
        std::sync::atomic::Ordering::SeqCst,
    );
    let (status, headers, body) = call(&app, "/get-session", None, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(expires(&body["session"]), now + Duration::seconds(150));
    assert!(
        headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=120")
    );
    // Rotations renew the configured TTL; factor builders must retain policy.
    let (status, headers, body) = call(
        &app,
        "/two-factor/enable",
        Some(json!({"password":"owned-password"})),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    cookie = cookies(&headers);
    let backup = body["backupCodes"].as_array().unwrap().clone();
    let (_, _, body) = call(&app, "/get-session", None, &cookie).await;
    assert_eq!(expires(&body["session"]), now + Duration::seconds(150));
    for (index, persistent) in [false, true].into_iter().enumerate() {
        let (status, challenge_headers, challenge_body) = call(
            &app,
            "/sign-in/email",
            Some(json!({"email":email,"password":"owned-password","rememberMe":persistent})),
            "",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(challenge_body["twoFactorRedirect"], true);
        let challenge = cookies(&challenge_headers);
        let (status, session_headers, body) = call(
            &app,
            "/two-factor/verify-backup-code",
            Some(json!({"code":backup[index]})),
            &challenge,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (_, _, view) = call(&app, "/get-session", None, &cookies(&session_headers)).await;
        assert_eq!(
            expires(&view["session"]),
            now + Duration::seconds(30 + if persistent { 120 } else { 86400 })
        );
    }
    // OTP completion uses a separate issuance branch; exercise its transient TTL too.
    let (_, challenge_headers, _) = call(
        &app,
        "/sign-in/email",
        Some(json!({"email":email,"password":"owned-password","rememberMe":false})),
        "",
    )
    .await;
    let challenge = cookies(&challenge_headers);
    let (status, _, body) = call(&app, "/two-factor/send-otp", Some(json!({})), &challenge).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let code = delivery.0.lock().unwrap().clone();
    let (status, session_headers, body) = call(
        &app,
        "/two-factor/verify-otp",
        Some(json!({"code":code})),
        &challenge,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, _, view) = call(&app, "/get-session", None, &cookies(&session_headers)).await;
    assert_eq!(
        expires(&view["session"]),
        now + Duration::seconds(30 + 86400)
    );
    let (_, headers, _) = call(
        &app,
        "/two-factor/disable",
        Some(json!({"password":"owned-password"})),
        &cookie,
    )
    .await;
    cookie = cookies(&headers);
    let (status,headers,body)=call(&app,"/change-password",Some(json!({"currentPassword":"owned-password","newPassword":"next-password","revokeOtherSessions":true})),&cookie).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    cookie = cookies(&headers);
    let (_, _, body) = call(&app, "/get-session", None, &cookie).await;
    assert_eq!(expires(&body["session"]), now + Duration::seconds(150));
    let (status, headers, body) = call(
        &app,
        "/sign-in/email",
        Some(json!({"email":email,"password":"next-password","rememberMe":false})),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=")
    );
    let mut tx = store.begin(false).await.unwrap();
    let row = tx
        .find_unique(Model::Session, "token", body["token"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        expires(&row),
        now + Duration::seconds(30) + Duration::days(1)
    );
    // Auto verification creates a configured-duration session; the transient cookie marker only affects cookie persistence here.
    let token = crate::accounts::email_delivery::token(email, &[7; 32], now).unwrap();
    let (status, headers, _) = call(
        &app,
        &format!("/verify-email?token={}", token.as_str()),
        None,
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    cookie = cookies(&headers);
    let (_, _, body) = call(&app, "/get-session", None, &cookie).await;
    assert_eq!(expires(&body["session"]), now + Duration::seconds(150));
    let google = crate::oauth::google_accounts::GoogleAccounts::new(store.clone())
        .with_session_policy(policy);
    let grant=google.sign_in_verified(&json!({"sub":"policy-google","email":"policy-google@example.test","email_verified":true,"given_name":"Owned","family_name":"Google"}),&json!({}),"","",now).await.unwrap();
    assert_eq!(expires(&grant.auth.session), now + Duration::seconds(120));
}
#[tokio::test]
async fn session_policy_http_all_creation_families_and_cookies() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
