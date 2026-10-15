use super::*;
use crate::store::{AuthTransaction, Model, memory::MemoryAuthStore};
use axum::body::to_bytes;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::sync::atomic::{AtomicI64, Ordering};
use tower::ServiceExt;
pub(crate) mod organization;
mod store;
use store::{Advance, Event, Store};
struct Delivery {
    clock: Arc<AtomicI64>,
    at: i64,
}
impl crate::accounts::EmailVerificationDelivery for Delivery {
    fn prepare(
        &self,
        _: crate::accounts::EmailVerificationDeliveryContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        let clock = self.clock.clone();
        let at = self.at;
        Ok(Box::pin(async move {
            tokio::task::yield_now().await;
            clock.store(at, Ordering::SeqCst);
            Ok(())
        }))
    }
}
#[derive(Default)]
struct Captured(std::sync::Mutex<String>);
impl crate::accounts::EmailVerificationDelivery for Captured {
    fn prepare(
        &self,
        input: crate::accounts::EmailVerificationDeliveryContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        *self.0.lock().unwrap() = input.token().to_owned();
        Ok(Box::pin(async { Ok(()) }))
    }
}
impl crate::accounts::PasswordResetDelivery for Captured {
    fn prepare(
        &self,
        input: crate::accounts::PasswordResetDeliveryContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        *self.0.lock().unwrap() = input.token().to_owned();
        Ok(Box::pin(async { Ok(()) }))
    }
}
async fn call(
    app: &Router,
    path: &str,
    payload: Option<Value>,
    cookie: &str,
) -> (StatusCode, HeaderMap, Value) {
    let request = Request::builder()
        .method(if payload.is_some() { "POST" } else { "GET" })
        .uri(path)
        .header("origin", "http://localhost:3000")
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .body(Body::from(
            payload.map_or_else(String::new, |p| p.to_string()),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1_048_576).await.unwrap();
    (
        parts.status,
        parts.headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn cookie(headers: &HeaderMap) -> String {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|h| h.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ")
}
fn time(row: &Value, field: &str) -> i64 {
    DateTime::parse_from_rfc3339(row[field].as_str().unwrap())
        .unwrap()
        .timestamp_millis()
}
async fn session<S: AuthStore>(store: &S, token: &str) -> Value {
    let mut tx = store.begin(false).await.unwrap();
    let row = tx
        .find_unique(Model::Session, "token", token)
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    row
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn lifecycle<S: AuthStore + 'static>(inner: Arc<S>) {
    let start = 1_800_000_000_000;
    let now = DateTime::from_timestamp_millis(start).unwrap();
    let clock = Arc::new(AtomicI64::new(start));
    let advance = Advance {
        clock: clock.clone(),
        armed: Arc::new(std::sync::Mutex::new(None)),
    };
    let store = Arc::new(Store {
        inner: inner.clone(),
        advance: advance.clone(),
    });
    let policy = crate::sessions::SessionPolicy::new(120, 30, 60, false).unwrap();
    let make = |after: bool| {
        let app = AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_session_policy(policy)
        .with_rate_limiting(false);
        let clock = clock.clone();
        let app = if after {
            app.with_two_factor().unwrap().with_clock(move || {
                DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap()
            })
        } else {
            app.with_clock(move || {
                DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap()
            })
            .with_two_factor()
            .unwrap()
        };
        app.with_two_factor_lifecycle_options(
            crate::two_factor::TwoFactorLifecycleOptions::default()
                .with_skip_verification_on_enable(true),
        )
        .unwrap()
    };
    let app = make(false).router();
    advance.arm(Event::Insert(Model::Account), start + 7_000);
    let(status,headers,body)=call(&app,"/sign-up/email",Some(json!({"email":"clock-policy@example.test","password":"owned-password","name":"Owned","firstName":"Clock","lastName":"Policy"})),"").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = body["token"].as_str().unwrap().to_owned();
    let original_cookie = cookie(&headers);
    let row = session(inner.as_ref(), &token).await;
    assert_eq!(time(&row, "createdAt"), start + 7_000);
    assert_eq!(time(&row, "expiresAt"), start + 127_000);
    clock.store(start, Ordering::SeqCst);
    advance.arm(Event::Get(Model::User), start + 9_000);
    let (status, signin_headers, body) = call(
        &app,
        "/sign-in/email",
        Some(json!({"email":"clock-policy@example.test","password":"owned-password"})),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        time(
            &session(inner.as_ref(), body["token"].as_str().unwrap()).await,
            "createdAt"
        ),
        start + 9_000
    );
    clock.store(start + 7_000, Ordering::SeqCst);
    advance.arm(Event::Get(Model::User), start + 38_000);
    let (status, _, body) = call(&app, "/get-session", None, &original_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(time(&body["session"], "expiresAt"), start + 158_000);
    // A direct service continues to honor its explicit timestamp despite wall-clock movement in the store.
    advance.arm(Event::Get(Model::User), start + 300_000);
    let direct = crate::sessions::SessionService::new(store.clone())
        .with_session_policy(policy)
        .get(&token, false, true, now)
        .await
        .unwrap();
    assert!(direct.is_some());
    // Same delayed read through HTTP rejects and commits expired-session cleanup.
    clock.store(start, Ordering::SeqCst);
    advance.arm(Event::Get(Model::User), start + 300_000);
    let (status, _, body) = call(&app, "/get-session", None, &original_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null());
    clock.store(start + 40_000, Ordering::SeqCst);
    advance.arm(Event::Replace(Model::Session), start + 300_000);
    let (status, _, body) = call(&app, "/get-session", None, &cookie(&signin_headers)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null());
    let captured = Arc::new(Captured::default());
    let delivery_app = make(false)
        .with_password_reset_delivery(captured.clone())
        .with_email_verification_delivery(captured.clone())
        .with_token_lifetimes(crate::accounts::AuthTokenLifetimes::new(120, 120))
        .router();
    clock.store(start, Ordering::SeqCst);
    advance.arm(Event::Find(Model::User), start + 19_000);
    let (status, _, body) = call(
        &delivery_app,
        "/request-password-reset",
        Some(json!({"email":"clock-policy@example.test"})),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let identifier = format!("reset-password:{}", captured.0.lock().unwrap());
    let mut tx = inner.begin(false).await.unwrap();
    let proof = tx
        .find_page(Model::Verification, "identifier", &identifier, None, 1)
        .await
        .unwrap();
    assert_eq!(time(&proof.records[0], "expiresAt"), start + 139_000);
    tx.commit().await.unwrap();
    clock.store(start, Ordering::SeqCst);
    advance.arm(Event::Find(Model::User), start + 23_000);
    let (status, _, body) = call(
        &delivery_app,
        "/send-verification-email",
        Some(json!({"email":"clock-policy@example.test"})),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let proof = captured.0.lock().unwrap().clone();
    let claims: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(proof.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(claims["exp"].as_i64().unwrap() * 1000, start + 143_000);
    // The awaited signup delivery controls session issuance time, not its original request timestamp.
    clock.store(start, Ordering::SeqCst);
    let delivery = Arc::new(Delivery {
        clock: clock.clone(),
        at: start + 11_000,
    });
    let hooked = make(false)
        .with_email_verification_delivery(delivery)
        .with_email_verification_policy(crate::accounts::EmailVerificationPolicy {
            send_on_signup: Some(true),
            ..Default::default()
        })
        .router();
    let(status,_,body)=call(&hooked,"/sign-up/email",Some(json!({"email":"clock-hook@example.test","password":"owned-password","name":"Owned","firstName":"Clock","lastName":"Hook"})),"").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        time(
            &session(inner.as_ref(), body["token"].as_str().unwrap()).await,
            "createdAt"
        ),
        start + 11_000
    );
    // Both builder orders preserve the HTTP clock through factor config copies and password derivation.
    for after in [false, true] {
        clock.store(start, Ordering::SeqCst);
        let app = make(after).router();
        let email = format!("clock-factor-{after}@example.test");
        let(status,headers,body)=call(&app,"/sign-up/email",Some(json!({"email":email,"password":"owned-password","name":"Owned","firstName":"Clock","lastName":"Factor"})),"").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let browser = cookie(&headers);
        advance.arm(Event::Insert(Model::TwoFactor), start + 13_000);
        let (status, headers, body) = call(
            &app,
            "/two-factor/enable",
            Some(json!({"password":"owned-password"})),
            &browser,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let recovery = body["backupCodes"][0].as_str().unwrap().to_owned();
        let (status, _, body) = call(&app, "/get-session", None, &cookie(&headers)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(time(&body["session"], "createdAt"), start + 13_000);
        {
            let (status, challenge_headers, body) = call(
                &app,
                "/sign-in/email",
                Some(json!({"email":email,"password":"owned-password"})),
                "",
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body["twoFactorRedirect"], true);
            let mut tx = inner.begin(false).await.unwrap();
            let proofs = tx.list(Model::Verification).await.unwrap();
            let factors = tx.list(Model::TwoFactor).await.unwrap();
            tx.commit().await.unwrap();
            advance.arm(
                if after {
                    Event::Insert(Model::Session)
                } else {
                    Event::Delete(Model::Verification)
                },
                start + 3_600_000,
            );
            let (status, headers, body) = call(
                &app,
                "/two-factor/verify-backup-code",
                Some(json!({"code":recovery})),
                &cookie(&challenge_headers),
            )
            .await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
            assert!(headers.get(header::SET_COOKIE).is_none());
            let mut tx = inner.begin(false).await.unwrap();
            assert_eq!(tx.list(Model::Verification).await.unwrap(), proofs);
            assert_eq!(tx.list(Model::TwoFactor).await.unwrap(), factors);
            tx.commit().await.unwrap();
        }
    }
    // Expiration during authenticated writes prevents password rotation and rolls the password mutation back.
    clock.store(start, Ordering::SeqCst);
    let app = make(false).router();
    let(status,headers,body)=call(&app,"/sign-up/email",Some(json!({"email":"clock-expiry@example.test","password":"owned-password","name":"Owned","firstName":"Clock","lastName":"Expiry"})),"").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let browser = cookie(&headers);
    let user = body["user"]["id"].as_str().unwrap().to_owned();
    let mut tx = inner.begin(false).await.unwrap();
    let before = crate::store::find_all(&mut tx, Model::Account, "userId", &user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    advance.arm(Event::Replace(Model::Account), start + 121_000);
    let(status,headers,body)=call(&app,"/change-password",Some(json!({"currentPassword":"owned-password","newPassword":"replacement-password","revokeOtherSessions":true})),&browser).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert!(headers.get(header::SET_COOKIE).is_none());
    let mut tx = inner.begin(false).await.unwrap();
    assert_eq!(
        crate::store::find_all(&mut tx, Model::Account, "userId", &user)
            .await
            .unwrap(),
        before
    );
    tx.commit().await.unwrap();
    // Trusted Google resolution uses the same post-account-write clock without adding a provider network call.
    clock.store(start, Ordering::SeqCst);
    advance.arm(Event::Insert(Model::Account), start + 17_000);
    let google = crate::oauth::google_accounts::GoogleAccounts::new(store.clone())
        .with_session_policy(policy)
        .with_time_source(crate::time_source::TimeSource::http({
            let clock = clock.clone();
            Arc::new(move || DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap())
        }));
    let grant=google.sign_in_verified(&json!({"sub":"clock-google","email":"clock-google@example.test","email_verified":true,"name":"Owned"}),&json!({}),"","",now).await.unwrap();
    assert_eq!(time(&grant.auth.session, "createdAt"), start + 17_000);
}
#[tokio::test]
async fn fresh_http_clock_delayed_reads_hooks_rotation_and_expired_authority() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}

#[tokio::test]
async fn fresh_http_clock_change_email_rechecks_after_writer_user_read() {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let start = 1_800_000_000_000;
    let clock = Arc::new(AtomicI64::new(start));
    let inner = Arc::new(MemoryAuthStore::default());
    let advance = Advance {
        clock: clock.clone(),
        armed: Arc::new(std::sync::Mutex::new(None)),
    };
    let store = Arc::new(Store {
        inner: inner.clone(),
        advance: advance.clone(),
    });
    let app = AuthHttp::new(store, "http://localhost:3000".into(), vec![7; 32], vec![])
        .unwrap()
        .with_session_policy(crate::sessions::SessionPolicy::new(120, 30, 60, false).unwrap())
        .with_rate_limiting(false)
        .with_clock(move || DateTime::from_timestamp_millis(clock.load(Ordering::SeqCst)).unwrap())
        .router();
    let (status, headers, body) = call(
        &app,
        "/sign-up/email",
        Some(json!({"email":"clock-change@example.test","password":"owned-password","name":"Owned","firstName":"Clock","lastName":"Change"})),
        "",
    ).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let session_token = body["token"].as_str().unwrap();
    let initial_session = session(inner.as_ref(), session_token).await;
    let claims = json!({"email":"clock-change@example.test","updateTo":"clock-changed@example.test","requestType":"change-email-verification","exp":start/1000+600});
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(&[7; 32]).unwrap();
    mac.update(input.as_bytes());
    let proof = format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    );
    // The initial active-session lookup succeeds. Only the subsequent writer's
    // target-user lookup advances time beyond that session's original expiry.
    advance.arm_after(Event::Get(Model::User), start + 121_000, 1);
    let (status, headers, body) = call(
        &app,
        &format!("/verify-email?token={proof}"),
        None,
        &cookie(&headers),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["code"], "INVALID_USER");
    assert!(!headers.contains_key(header::SET_COOKIE));
    assert_eq!(
        session(inner.as_ref(), session_token).await,
        initial_session
    );
    let mut tx = inner.begin(false).await.unwrap();
    assert!(
        tx.find_unique(Model::User, "email", "clock-change@example.test")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        tx.find_unique(Model::User, "email", "clock-changed@example.test")
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
}
