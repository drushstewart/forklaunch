use super::*;
use crate::{
    accounts::{AuthTokenLifetimes, CredentialPolicy},
    store::{AuthTransaction, Model, memory::MemoryAuthStore},
};
use axum::body::to_bytes;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use tower::ServiceExt;
#[derive(Default)]
struct Delivery(std::sync::Mutex<Vec<String>>);
impl crate::accounts::EmailVerificationDelivery for Delivery {
    fn prepare(
        &self,
        input: crate::accounts::EmailVerificationDeliveryContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        self.0.lock().unwrap().push(input.token().to_owned());
        Ok(Box::pin(async { Ok(()) }))
    }
}
impl crate::accounts::PasswordResetDelivery for Delivery {
    fn prepare(
        &self,
        input: crate::accounts::PasswordResetDeliveryContext,
    ) -> BlueprintResult<crate::background::DeliveryFuture> {
        self.0.lock().unwrap().push(input.token().to_owned());
        Ok(Box::pin(async { Ok(()) }))
    }
}
async fn call(
    router: &Router,
    path: &str,
    body: Value,
    cookie: &str,
) -> (StatusCode, HeaderMap, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("origin", "http://localhost:3000")
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1_048_576).await.unwrap();
    (
        parts.status,
        parts.headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn signup(email: &str, password: &str) -> Value {
    json!({"email":email,"password":password,"name":"Owned","firstName":"Policy","lastName":"Owned"})
}
#[allow(clippy::too_many_lines)]
pub(crate) async fn lifecycle<S: AuthStore + 'static>(store: Arc<S>) {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let reset = Arc::new(Delivery::default());
    let email = Arc::new(Delivery::default());
    let make = |policy| {
        AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            vec![7; 32],
            vec![],
        )
        .unwrap()
        .with_two_factor()
        .unwrap()
        .with_credential_policy(policy)
        .with_password_reset_delivery(reset.clone())
        .with_email_verification_delivery(email.clone())
        .with_token_lifetimes(AuthTokenLifetimes::new(120, 1))
        .with_rate_limiting(false)
        .with_clock(move || now)
        .router()
    };
    let disabled = make(CredentialPolicy::new(true, 4, 4).unwrap());
    for (password, code) in [
        ("abc", "EMAIL_PASSWORD_SIGN_UP_DISABLED"),
        ("", "VALIDATION_ERROR"),
    ] {
        let (status, _, body) = call(
            &disabled,
            "/sign-up/email",
            signup("credential-policy@example.test", password),
            "",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], code);
    }
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.find_unique(Model::User, "email", "credential-policy@example.test")
            .await
            .unwrap()
            .is_none()
    );
    tx.commit().await.unwrap();
    let automatic = AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![7; 32],
        vec![],
    )
    .unwrap()
    .with_credential_policy(CredentialPolicy::new(false, 4, 4).unwrap())
    .with_token_lifetimes(AuthTokenLifetimes::new(120, 2))
    .with_email_verification_delivery(email.clone())
    .with_email_verification_policy(crate::accounts::EmailVerificationPolicy {
        require: true,
        send_on_signup: None,
        send_on_signin: true,
    })
    .with_clock(move || now)
    .with_rate_limiting(false)
    .router();
    let (status, _, body) = call(
        &automatic,
        "/sign-up/email",
        signup("automatic-credential-policy@example.test", "abcd"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["token"].is_null());
    let (status, _, body) = call(
        &automatic,
        "/sign-in/email",
        json!({"email":"automatic-credential-policy@example.test","password":"abcd"}),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    {
        let mut sent = email.0.lock().unwrap();
        assert_eq!(sent.len(), 2);
        for token in sent.iter() {
            let claims: Value = serde_json::from_slice(
                &URL_SAFE_NO_PAD
                    .decode(token.split('.').nth(1).unwrap())
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
                2
            );
        }
        sent.clear();
    }
    let app = make(CredentialPolicy::new(false, 4, 4).unwrap());
    for (password, code) in [
        ("abc", "PASSWORD_TOO_SHORT"),
        ("abcde", "PASSWORD_TOO_LONG"),
    ] {
        let (status, _, body) = call(
            &app,
            "/sign-up/email",
            signup("credential-policy@example.test", password),
            "",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], code);
    }
    let (status, headers, body) = call(
        &app,
        "/sign-up/email",
        signup("credential-policy@example.test", "😀😀"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let credentials = crate::accounts::CredentialService::new(store.clone())
        .with_credential_policy(CredentialPolicy::new(false, 4, 4).unwrap());
    for (password, code) in [
        ("abc", "PASSWORD_TOO_SHORT"),
        ("abcde", "PASSWORD_TOO_LONG"),
    ] {
        assert_eq!(
            credentials
                .set_password(body["token"].as_str().unwrap(), password.into(), false, now)
                .await
                .unwrap_err()
                .code,
            code
        );
    }
    let cookie = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    for (route, payload) in [
        (
            "/sign-in/email",
            json!({"email":"credential-policy@example.test","password":"abcde"}),
        ),
        ("/verify-password", json!({"password":"abcde"})),
        ("/two-factor/enable", json!({"password":"abcde"})),
        (
            "/change-password",
            json!({"currentPassword":"😀😀","newPassword":"abcde"}),
        ),
    ] {
        let (_, _, body) = call(&app, route, payload, &cookie).await;
        assert_eq!(body["code"], "PASSWORD_TOO_LONG", "{route}: {body}");
    }
    assert!(email.0.lock().unwrap().is_empty());
    let (status, _, body) = call(
        &app,
        "/send-verification-email",
        json!({"email":"credential-policy@example.test"}),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let proof = email.0.lock().unwrap()[0].clone();
    let claims: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(proof.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
        1
    );
    let (status, _, body) = call(
        &app,
        "/request-password-reset",
        json!({"email":"credential-policy@example.test"}),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = reset.0.lock().unwrap()[0].clone();
    let identifier = format!("reset-password:{token}");
    let mut tx = store.begin(false).await.unwrap();
    let row = tx
        .find_page(Model::Verification, "identifier", &identifier, None, 1)
        .await
        .unwrap();
    assert_eq!(
        DateTime::parse_from_rfc3339(row.records[0]["expiresAt"].as_str().unwrap())
            .unwrap()
            .timestamp(),
        now.timestamp() + 120
    );
    tx.commit().await.unwrap();
    let (_, _, body) = call(
        &app,
        "/reset-password",
        json!({"token":token,"newPassword":"abc"}),
        "",
    )
    .await;
    assert_eq!(body["code"], "PASSWORD_TOO_SHORT");
    let (status, _, body) = call(
        &app,
        "/reset-password",
        json!({"token":token,"newPassword":"abcd"}),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _, body) = call(
        &app,
        "/sign-in/email",
        json!({"email":"credential-policy@example.test","password":"abcd"}),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
#[tokio::test]
async fn configured_credential_policy_http_lifecycle() {
    lifecycle(Arc::new(MemoryAuthStore::default())).await;
}
#[test]
fn bounded_credential_policy_and_token_lifetimes() {
    assert!(CredentialPolicy::new(false, 4097, 128).is_err());
    assert_eq!(
        CredentialPolicy::new(false, 0, 0).unwrap(),
        CredentialPolicy::default()
    );
    assert_eq!(AuthTokenLifetimes::new(0, 0).password_reset_seconds(), 3600);
    assert_eq!(
        AuthTokenLifetimes::new(0, 0).email_verification_seconds(),
        0
    );
    let inverted = CredentialPolicy::new(false, 10, 5).unwrap();
    assert_eq!(
        inverted.validate("1234567", true).unwrap_err().code,
        "PASSWORD_TOO_SHORT"
    );
    assert_eq!(
        inverted.validate("1234567890", true).unwrap_err().code,
        "PASSWORD_TOO_LONG"
    );
    assert!(
        crate::accounts::email_delivery::token_with_lifetime(
            "x@example.test",
            &[7; 32],
            DateTime::<Utc>::MAX_UTC,
            u32::MAX
        )
        .is_err()
    );
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let token =
        crate::accounts::email_delivery::token_with_lifetime("x@example.test", &[7; 32], now, 0)
            .unwrap();
    let claims: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(token.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(claims["exp"], claims["iat"]);
}
